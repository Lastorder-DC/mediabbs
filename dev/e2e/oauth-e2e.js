// End-to-end test of the dev stack (docker-compose.yml): OAuth login through
// the wiki, moderation, blocks, uploads and anonymous posting.
//   cd dev/e2e && npm install && npx playwright install chromium-headless-shell
//   node oauth-e2e.js
const { chromium } = require("playwright");
const BBS = "http://127.0.0.1:18090", API = "http://127.0.0.1:18081";
const results = [];
const check = (name, ok, detail = "") => { results.push([ok ? "PASS" : "FAIL", name, detail]); };

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

async function loginAs(browser, user, pass) {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto(BBS + "/free", { waitUntil: "networkidle" });
  await page.click(".user-items a.navbar-item");
  await page.waitForSelector("#wpName1");
  await page.fill("#wpName1", user);
  await page.fill("#wpPassword1", pass);
  await page.click("#wpLoginAttempt");
  await page.waitForLoadState("networkidle");
  if (page.url().includes("approve")) {
    await page.click("button[name=accept]");
  }
  await page.waitForURL(BBS + "/free", { timeout: 15000 });
  await page.waitForLoadState("networkidle");
  return { ctx, page };
}

(async () => {
  const browser = await chromium.launch();
  const PW = "mediabbs-dev-user";

  // --- Alice: regular user ---
  const alice = await loginAs(browser, "Alice", PW);
  const cookies = await alice.ctx.cookies(API);
  check("Alice: auth cookies set (HttpOnly)", cookies.filter((c) => c.httpOnly && /_token$/.test(c.name)).length === 2,
        cookies.map((c) => c.name).join(","));
  let me = await api(alice.page, "GET", "/me");
  check("Alice: /me", me.status === 200 && me.json.username === "Alice" && me.json.isAdmin === false && me.json.blocked === false,
        me.text.slice(0, 160));
  check("Alice: navbar shows user menu", await alice.page.isVisible(".user-gravatar-wrapper"));
  const topic = await api(alice.page, "POST", "/topics", { boardId: 1, title: "Alice의 글", content: "로그인 사용자 글" });
  check("Alice: create topic", topic.status === 200 && topic.json.authorName === "Alice" && topic.json.authorId > 0, topic.text.slice(0, 120));
  const c = await api(alice.page, "POST", `/topics/${topic.json.id}/comments`, { content: "댓글" });
  check("Alice: comment", c.status === 204, String(c.status));
  const png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
  const up = await api(alice.page, "POST", "/files", { filename: "dot.png", content: png });
  check("Alice: upload (login required)", up.status === 200 && /^http:\/\/127\.0\.0\.1:18081\/uploads\//.test(up.json.url), up.text);
  const rf = await api(alice.page, "POST", "/auth/refresh");
  me = await api(alice.page, "GET", "/me");
  check("Alice: refresh token -> new access token works", rf.status === 204 && me.status === 200, `${rf.status}/${me.status}`);
  const st = await api(alice.page, "PUT", `/topics/${topic.json.id}/status`, { isPinned: true });
  check("Alice: moderation forbidden", st.status === 403, String(st.status));
  await alice.page.goto(`${BBS}/free/${topic.json.id}`, { waitUntil: "networkidle" });
  check("Alice: no admin menu", !(await alice.page.isVisible(".admin-tools")));

  // --- Admin (sysop) ---
  const admin = await loginAs(browser, "Admin", "mediabbs-dev-admin");
  me = await api(admin.page, "GET", "/me");
  check("Admin: isAdmin + email (authonlyprivate grant)", me.json.isAdmin === true && me.json.email === "admin@example.invalid", me.text.slice(0, 200));
  await admin.page.goto(`${BBS}/free/${topic.json.id}`, { waitUntil: "networkidle" });
  check("Admin: admin menu visible", await admin.page.isVisible(".admin-tools"));
  const comments = await api(admin.page, "GET", `/topics/${topic.json.id}/comments`);
  const hid = await api(admin.page, "PUT", `/comments/${comments.json[1].id}/status`, { isHidden: true });
  check("Admin: hide comment", hid.status === 200 && hid.json.isHidden === true, hid.text.slice(0, 100));
  const shown = await api(admin.page, "GET", `/comments/${comments.json[1].id}?showHidden=true`);
  check("Admin: view hidden comment", shown.status === 200 && shown.json.content === "댓글", shown.text.slice(0, 100));
  const anonView = await api(alice.page, "GET", `/comments/${comments.json[1].id}?showHidden=true`);
  check("Alice: cannot view hidden comment", anonView.status === 403, String(anonView.status));
  const pin = await api(admin.page, "PUT", `/topics/${topic.json.id}/status`, { isPinned: true });
  check("Admin: pin topic", pin.status === 200 && pin.json.isPinned === true, String(pin.status));
  await api(admin.page, "PUT", `/topics/${topic.json.id}/status`, { isPinned: false }); // keep the board tidy

  // --- Moderator (boardmanager group via ADMIN_GROUPS) ---
  const mod = await loginAs(browser, "Moderator", PW);
  me = await api(mod.page, "GET", "/me");
  check("Moderator: isAdmin via ADMIN_GROUPS=boardmanager", me.json.isAdmin === true && me.json.groups.includes("boardmanager"), JSON.stringify(me.json.groups));

  // --- Blocked user ---
  const blocked = await loginAs(browser, "Blocked", PW);
  me = await api(blocked.page, "GET", "/me");
  check("Blocked: /me blocked=true", me.json.blocked === true, me.text.slice(0, 120));
  const bt = await api(blocked.page, "POST", "/topics", { boardId: 1, title: "x", content: "y" });
  check("Blocked: create topic rejected", bt.status === 403 && bt.text === "You are blocked", `${bt.status} ${bt.text}`);
  await blocked.page.goto(`${BBS}/free/${topic.json.id}`, { waitUntil: "networkidle" });
  const msg = await blocked.page.$eval(".page-topic", (e) => e.innerText).catch(() => "");
  check("Blocked: UI explains block", msg.includes("차단되어"), "");

  // --- Logout ---
  const lo = await api(alice.page, "POST", "/auth/logout");
  me = await api(alice.page, "GET", "/me");
  check("Alice: logout clears session", lo.status === 204 && me.status === 401, `${lo.status}/${me.status}`);

  // --- Anonymous ---
  const anonCtx = await browser.newContext();
  const anon = await anonCtx.newPage();
  await anon.goto(BBS + "/free", { waitUntil: "networkidle" });
  const at = await api(anon, "POST", "/topics", { boardId: 1, title: "익명 글", content: "익명" });
  check("Anonymous: create topic shows IP", at.status === 200 && /^[0-9a-f.:]+$/.test(at.json.authorName), at.json && at.json.authorName);
  const aup = await api(anon, "POST", "/files", { filename: "dot.png", content: png });
  check("Anonymous: upload requires login", aup.status === 401 && aup.text === "LoginRequired", `${aup.status} ${aup.text}`);
  const arch = await api(anon, "POST", "/topics", { boardId: 2, title: "x", content: "y" });
  check("Anonymous: disabled board rejected", arch.status === 403, `${arch.status} ${arch.text}`);
  console.log("anonymous IP as seen by the backend:", at.json && at.json.authorName);

  await browser.close();
  for (const r of results) console.log(r.join("  |  "));
  const ok = results.every((r) => r[0] === "PASS");
  console.log(ok ? "ALL PASSED" : "SOME FAILED");
  process.exit(ok ? 0 : 1);
})().catch((e) => { console.error(e); for (const r of results) console.log(r.join("  |  ")); process.exit(1); });
