// Security checks on the dev stack: sanitizing, hidden content, CORS, login
// CSRF, menu links, security headers.
//   node security-e2e.js
const { chromium } = require("playwright");
const BBS = "http://127.0.0.1:18090", API = "http://127.0.0.1:18081";
const results = [];
const check = (name, ok, detail = "") => results.push([ok ? "PASS" : "FAIL", name, detail]);

async function api(page, method, path, body) {
  return page.evaluate(async ([API, method, path, body]) => {
    const r = await fetch(API + path, {
      method, credentials: "include",
      headers: body ? { "Content-Type": "application/json" } : {},
      body: body ? JSON.stringify(body) : undefined,
    });
    const text = await r.text();
    let json = null; try { json = JSON.parse(text); } catch {}
    return { status: r.status, text, json };
  }, [API, method, path, body]);
}

async function login(browser, user, pass) {
  const ctx = await browser.newContext({ locale: "ko-KR" });
  const page = await ctx.newPage();
  await page.goto(BBS + "/free", { waitUntil: "networkidle" });
  await page.click(".user-items a.navbar-item");
  await page.waitForSelector("#wpName1");
  await page.fill("#wpName1", user);
  await page.fill("#wpPassword1", pass);
  await page.click("#wpLoginAttempt");
  await page.waitForLoadState("networkidle");
  if (page.url().includes("approve")) await page.click("button[name=accept]");
  await page.waitForURL(BBS + "/free", { timeout: 15000 });
  await page.waitForLoadState("networkidle");
  return { ctx, page };
}

(async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ locale: "ko-KR" });
  const page = await ctx.newPage();
  const cspViolations = [];
  page.on("console", (m) => { if (/Content Security Policy/i.test(m.text())) cspViolations.push(m.text()); });
  await page.goto(BBS + "/free", { waitUntil: "networkidle" });

  // --- client security headers ---
  const res = await page.request.get(BBS + "/free");
  const csp = res.headers()["content-security-policy"] || "";
  check("Client sends a CSP (scripts from self, no framing)", csp.includes("script-src 'self'") && csp.includes("frame-ancestors 'none'"), csp);
  check("Client forbids framing (X-Frame-Options)", res.headers()["x-frame-options"] === "DENY");
  const asset = await page.request.get(BBS + "/config.js");
  check("Headers on every location (config.js too)", (asset.headers()["content-security-policy"] || "").includes("frame-ancestors"));

  // --- comment HTML: no styles or forms, task lists still work ---
  const evil = '<div style="position:fixed;inset:0" class="evil">x</div><form action="https://evil.example/"><input name="pw" type="password"><button>로그인</button></form>\n\n- [x] 완료한 일';
  const topic = (await api(page, "POST", "/topics", { boardId: 1, title: `보안 ${Date.now()}`, content: evil })).json;
  await page.goto(`${BBS}/free/${topic.id}`, { waitUntil: "networkidle" });
  const body = page.locator(".topic-comment-card .md-content").first();
  const html = await body.innerHTML();
  check("Comments lose inline styles", !html.includes("style=") && !html.includes("position:fixed"), html.slice(0, 120));
  check("Comments lose forms, password fields and buttons", !/<form|type="password"|<button/.test(html), html.slice(0, 160));
  check("Task list checkboxes stay (disabled)", (await body.locator('input[type="checkbox"][disabled]').count()) === 1);

  // --- login CSRF: a callback this tab did not start is refused ---
  let tokenRequests = 0;
  page.on("request", (r) => { if (r.url().endsWith("/auth/login")) tokenRequests++; });
  await page.goto(`${BBS}/auth?code=attacker-code&state=%2Ffree`, { waitUntil: "networkidle" });
  check("Login callback without a matching state is refused", tokenRequests === 0 && (await page.textContent(".page-auth")).length > 0, `${tokenRequests} token requests`);

  // --- CORS: other origins get no credentials ---
  const cors = await fetch(API + "/me", { headers: { Origin: "https://evil.example" } });
  // Browsers only hand a response to a page whose origin is in Access-Control-Allow-Origin.
  check("CORS: other origins are not allowed", ![ "https://evil.example", "*" ].includes(cors.headers.get("access-control-allow-origin")),
    `${cors.headers.get("access-control-allow-origin")} / ${cors.headers.get("access-control-allow-credentials")}`);
  const ok = await fetch(API + "/me", { headers: { Origin: BBS } });
  check("CORS: the client origin gets credentials", ok.headers.get("access-control-allow-origin") === BBS && ok.headers.get("access-control-allow-credentials") === "true");

  // --- hidden topics stay hidden everywhere ---
  const admin = await login(browser, "Admin", "mediabbs-dev-admin");
  const comments = (await api(page, "GET", `/topics/${topic.id}/comments?limit=20&offset=0`)).json;
  check("Pagination: negative values are clamped, not errors", (await api(page, "GET", `/topics/${topic.id}/comments?limit=-1&offset=-5`)).status === 200);
  await api(admin.page, "PUT", `/topics/${topic.id}/status`, { isHidden: true });
  check("Hidden topic: its comments cannot be read one by one", (await api(page, "GET", `/comments/${comments[0].id}`)).status === 403);
  check("Hidden topic: admins still can (showHidden)", (await api(admin.page, "GET", `/comments/${comments[0].id}?showHidden=true`)).status === 200);
  check("Hidden topic: no live-update stream", (await api(page, "GET", `/topics/${topic.id}/events`)).status === 403);

  // --- menu links ---
  const badMenu = await api(admin.page, "PUT", "/admin/menus", { menus: { simple: [{ label: "x", url: "/\\evil.example" }] } });
  check("Menu links with backslashes are refused (/\\host = //host)", badMenu.status === 400, `${badMenu.status} ${badMenu.text}`);
  await admin.ctx.close();

  check("No CSP violations on the pages visited", cspViolations.length === 0, cspViolations.join(" | ").slice(0, 200));
  await browser.close();
  for (const r of results) console.log(r.join("  |  "));
  const failed = results.filter((r) => r[0] === "FAIL").length;
  console.log(failed ? `${failed} FAILED` : "ALL PASSED");
  process.exit(failed ? 1 : 0);
})().catch((e) => { for (const r of results) console.log(r.join("  |  ")); console.error(e); process.exit(1); });
