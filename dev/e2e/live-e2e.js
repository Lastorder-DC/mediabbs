// Live comment updates (Server-Sent Events) on the dev stack, and the polling
// fallback when the event stream is unavailable.
//   node live-e2e.js
const { chromium } = require("playwright");
const BBS = "http://127.0.0.1:18090", API = "http://127.0.0.1:18081";
const results = [];
const check = (name, ok, detail = "") => results.push([ok ? "PASS" : "FAIL", name, detail]);
/** Polling checks every 10 s, so anything faster than this came live. */
const LIVE_MS = 5000;

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

async function within(ms, fn) {
  const start = Date.now();
  while (Date.now() - start < ms) {
    if (await fn()) return Date.now() - start;
    await new Promise((r) => setTimeout(r, 200));
  }
  return null;
}

(async () => {
  const browser = await chromium.launch();
  const pageErrors = [];
  const viewerCtx = await browser.newContext({ locale: "ko-KR" });
  const viewer = await viewerCtx.newPage();
  viewer.on("pageerror", (e) => pageErrors.push(String(e)));
  await viewer.goto(BBS + "/free", { waitUntil: "networkidle" });
  const topic = (await api(viewer, "POST", "/topics", { boardId: 1, title: `실시간 ${Date.now()}`, content: "첫 의견" })).json;
  const topicUrl = `${BBS}/free/${topic.id}`;

  // --- the event stream itself ---
  const head = await viewer.evaluate(async ([API, id]) => {
    const controller = new AbortController();
    const r = await fetch(`${API}/topics/${id}/events`, { signal: controller.signal });
    const reader = r.body.getReader();
    const first = new TextDecoder().decode((await reader.read()).value);
    controller.abort();
    return { status: r.status, type: r.headers.get("content-type"), first };
  }, [API, topic.id]);
  check("Event stream answers with text/event-stream right away", head.status === 200 && head.type.startsWith("text/event-stream") && head.first.startsWith("retry: 5000"), JSON.stringify(head));
  check("Event stream for a missing topic: 404", (await api(viewer, "GET", "/topics/99999999/events")).status === 404);

  // --- Liberty viewer gets a new comment live ---
  await viewer.goto(topicUrl, { waitUntil: "networkidle" });
  await viewer.waitForSelector(".topic-comment-card");
  await viewer.waitForTimeout(500); // let the stream connect
  const poster = await browser.newContext({ locale: "ko-KR" });
  const posterPage = await poster.newPage();
  await posterPage.goto(BBS + "/free", { waitUntil: "networkidle" });
  await api(posterPage, "POST", `/topics/${topic.id}/comments`, { content: "다른 사람의 의견" });
  let ms = await within(LIVE_MS, async () => (await viewer.locator(".topic-comment-card").count()) === 2);
  check("Liberty: a new comment appears live", ms !== null, ms === null ? "not within 5 s" : `${ms} ms`);

  // --- moderation shows up live ---
  const admin = await login(browser, "Admin", "mediabbs-dev-admin");
  const comments = (await api(viewer, "GET", `/topics/${topic.id}/comments?limit=20&offset=0`)).json;
  await api(admin.page, "PUT", `/comments/${comments[1].id}/status`, { isHidden: true });
  ms = await within(LIVE_MS, async () => (await viewer.locator(".topic-comment-card >> nth=1").innerText()).includes("관리자에 의해 숨겨졌습니다"));
  check("A hidden comment is hidden live", ms !== null, ms === null ? "not within 5 s" : `${ms} ms`);
  await api(admin.page, "PUT", `/topics/${topic.id}/status`, { isClosed: true });
  ms = await within(LIVE_MS, async () => (await viewer.textContent("body")).includes("종료되어 의견을 추가할 수 없습니다"));
  check("Closing the topic shows live", ms !== null, ms === null ? "not within 5 s" : `${ms} ms`);
  await api(admin.page, "PUT", `/topics/${topic.id}/status`, { isClosed: false });
  await admin.ctx.close();

  // --- Simple skin too ---
  const simpleCtx = await browser.newContext({ locale: "ko-KR" });
  await simpleCtx.addInitScript(() => localStorage.setItem("mediabbs.skin", "simple"));
  const simple = await simpleCtx.newPage();
  simple.on("pageerror", (e) => pageErrors.push(String(e)));
  await simple.goto(topicUrl, { waitUntil: "networkidle" });
  await simple.waitForSelector(".s-comment");
  await simple.waitForTimeout(500);
  const before = await simple.locator(".s-comment").count();
  await api(posterPage, "POST", `/topics/${topic.id}/comments`, { content: "심플에서 볼 의견" });
  ms = await within(LIVE_MS, async () => (await simple.locator(".s-comment").count()) === before + 1);
  check("Simple: a new comment appears live", ms !== null, ms === null ? "not within 5 s" : `${ms} ms`);

  // --- background tab: stream closed, missed comments loaded on return ---
  await simple.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await api(posterPage, "POST", `/topics/${topic.id}/comments`, { content: "숨어 있는 동안 쓴 의견" });
  await simple.waitForTimeout(1000);
  const whileHidden = await simple.locator(".s-comment").count();
  await simple.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  ms = await within(LIVE_MS, async () => (await simple.locator(".s-comment").count()) === before + 2);
  check("Background tab: no stream; missed comments load on return", whileHidden === before + 1 && ms !== null, `hidden ${whileHidden}, back ${ms} ms`);
  await simpleCtx.close();

  // --- fallback: stream refused (503) -> polling every 10 s ---
  const pollCtx = await browser.newContext({ locale: "ko-KR" });
  const poll = await pollCtx.newPage();
  await poll.route("**/topics/*/events", (route) => route.fulfill({ status: 503, body: "Live updates are unavailable" }));
  await poll.goto(topicUrl, { waitUntil: "networkidle" });
  await poll.waitForSelector(".topic-comment-card");
  const pollBefore = await poll.locator(".topic-comment-card").count();
  await api(posterPage, "POST", `/topics/${topic.id}/comments`, { content: "폴링으로 볼 의견" });
  ms = await within(13000, async () => (await poll.locator(".topic-comment-card").count()) === pollBefore + 1);
  check("Without the stream, polling still brings new comments", ms !== null, ms === null ? "not within 13 s" : `${ms} ms`);
  await pollCtx.close();

  check("No uncaught page errors", pageErrors.length === 0, pageErrors.join(" | "));
  await browser.close();
  for (const r of results) console.log(r.join("  |  "));
  const failed = results.filter((r) => r[0] === "FAIL").length;
  console.log(failed ? `${failed} FAILED` : "ALL PASSED");
  process.exit(failed ? 1 : 0);
})().catch((e) => { for (const r of results) console.log(r.join("  |  ")); console.error(e); process.exit(1); });
