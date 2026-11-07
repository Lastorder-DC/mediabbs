// Browser test of the client UI on the dev stack (docker-compose.yml):
// editors, quoting, moderation menu, infinite scroll, i18n and skins.
//   node ui-e2e.js        (after oauth-e2e.js's setup steps; see README.md)
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

async function typeInEditor(page, text) {
  const ww = page.locator(".mb-editor .ProseMirror").first();
  await ww.click();
  await page.keyboard.type(text);
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
  const pageErrors = [];
  page.on("pageerror", (e) => pageErrors.push(String(e)));

  // --- Liberty: new topic through the editor modal ---
  await page.goto(BBS + "/free", { waitUntil: "networkidle" });
  check("Liberty skin (SKIN=liberty) is the default", await page.isVisible(".liberty-app"));
  const libertyMenu = (await page.locator(".liberty-navbar .navbar-start .navbar-text").allInnerTexts()).slice(1);
  check("Liberty menu: its default entries", JSON.stringify(libertyMenu) === JSON.stringify(["최근 바뀜", "임의 문서", "게시판", "도구"]), libertyMenu.join(","));
  await page.click(".liberty-tools .write-button");
  await page.waitForSelector(".editor-modal .mb-editor");
  const title = `UI 테스트 ${Date.now()}`;
  await page.fill(".editor-modal input.input", title);
  await typeInEditor(page, "첫 의견 본문");
  await page.click(".editor-modal .submit-button");
  await page.waitForURL(/\/free\/\d+$/, { timeout: 10000 });
  await page.waitForSelector(".topic-comment-card");
  const topicUrl = page.url();
  const topicId = Number(topicUrl.split("/").pop());
  check("Editor modal creates a topic", (await page.textContent(".topic-title")) === title, topicUrl);
  check("First comment rendered as Markdown", (await page.textContent(".topic-comment-card .md-content")).includes("첫 의견 본문"));

  // --- comment through the modal ---
  await page.click(".new-comment-button");
  await page.waitForSelector(".editor-modal .mb-editor");
  await typeInEditor(page, "두 번째 의견");
  await page.click(".editor-modal .submit-button");
  await page.waitForFunction(() => document.querySelectorAll(".topic-comment-card").length === 2, null, { timeout: 10000 });
  check("Comment modal posts and the list refreshes", !(await page.isVisible(".editor-modal")));

  // --- quote ---
  await page.locator(".topic-comment-card").nth(1).locator(".dropdown-trigger button").click();
  await page.click(".topic-comment-card >> nth=1 >> text=인용");
  await page.waitForSelector(".editor-modal .mb-editor");
  const quoted = await page.locator(".editor-modal .mb-editor .ProseMirror").innerText();
  check("Quote opens the editor with the quoted text", quoted.includes("두 번째 의견") && quoted.includes("님이 작성"), quoted.replace(/\n/g, " ⏎ ").slice(0, 80));
  await page.click(".editor-modal .delete");

  // --- editor: toolbar, Markdown mode, output ---
  await page.click(".new-comment-button");
  await page.waitForSelector(".editor-modal .mb-editor .ProseMirror");
  await typeInEditor(page, "보통 ");
  await page.click(".editor-modal .mb-editor-type-bold");
  await page.keyboard.type("굵게");
  await page.click(".editor-modal .mb-editor-type-bold");
  await page.keyboard.press("Enter");
  await page.click(".editor-modal .mb-editor-list-ul");
  await page.keyboard.type("항목");
  await page.click(".editor-modal .mb-editor-modes button >> nth=0");
  const source = await page.inputValue(".editor-modal .mb-editor-source");
  check("Editor: toolbar formatting comes out as Markdown", source.includes("보통 **굵게**") && source.includes("- 항목"), source.replace(/\n/g, " ⏎ "));
  await page.fill(".editor-modal .mb-editor-source", source + "\n\n| 가 | 나 |\n| --- | --- |\n| 1 | 2 |");
  await page.click(".editor-modal .mb-editor-modes button >> nth=1");
  check("Editor: Markdown typed in source mode shows in the WYSIWYG view", (await page.locator(".editor-modal .ProseMirror table td").count()) === 2);
  await page.click(".editor-modal .submit-button");
  await page.waitForFunction(() => document.querySelectorAll(".topic-comment-card").length === 3, null, { timeout: 10000 });
  const rendered = page.locator(".topic-comment-card >> nth=2 >> .md-content");
  check("Editor: posted Markdown renders (bold, list, table)",
    (await rendered.locator("strong").innerText()) === "굵게" && (await rendered.locator("li").count()) === 1 && (await rendered.locator("table td").count()) === 2);

  // --- viewer: TOAST UI-era line breaks and sanitizing ---
  const legacy = await api(page, "POST", `/topics/${topicId}/comments`, {
    content: "줄1\n줄2\n\n문단2 <img src=x onerror=\"window.__xss=1\"> [링크](javascript:window.__xss=2)",
  });
  check("Viewer test comment posted", legacy.status < 300, String(legacy.status));
  await page.reload({ waitUntil: "networkidle" });
  const legacyHtml = await page.locator(".topic-comment-card >> nth=3 >> .md-content").innerHTML();
  check("Viewer: single newlines stay line breaks (as TOAST UI wrote them)", /줄1<br>\s*줄2/.test(legacyHtml), legacyHtml.slice(0, 80));
  check("Viewer: scripts and javascript: links are removed",
    !legacyHtml.includes("onerror") && !legacyHtml.includes("javascript:") && (await page.evaluate(() => window.__xss)) === undefined);

  // --- error messages are translated ---
  await page.goto(BBS + "/free/99999999", { waitUntil: "networkidle" });
  check("Missing topic: translated error", (await page.textContent(".page-error")).includes("존재하지 않는 주제"));
  await page.goto(BBS + "/no-such-board", { waitUntil: "networkidle" });
  check("Missing board: translated error", (await page.textContent(".page-error")).includes("존재하지 않는 게시판"));

  // --- archived board: no writing ---
  await page.goto(BBS + "/archive", { waitUntil: "networkidle" });
  check("Archived board hides the write button", !(await page.isVisible(".liberty-tools .write-button")));

  // --- infinite scroll ---
  for (let i = 0; i < 22; i++) await api(page, "POST", "/topics", { boardId: 1, title: `스크롤 ${i}`, content: "x" });
  const all = (await api(page, "GET", "/boards/1/topics?limit=20&offset=0")).json.length;
  let total = 0;
  for (let offset = 0; ; offset += 20) {
    const n = (await api(page, "GET", `/boards/1/topics?limit=20&offset=${offset}`)).json.length;
    total += n;
    if (n < 20) break;
  }
  await page.goto(BBS + "/free", { waitUntil: "networkidle" });
  const firstPage = await page.locator(".topic-card").count();
  for (let i = 0, last = -1, still = 0; still < 5 && (await page.locator(".topic-card").count()) < total; i++) {
    await page.mouse.wheel(0, 5000);
    await page.waitForTimeout(300);
    const n = await page.locator(".topic-card").count();
    still = n === last ? still + 1 : 0;
    last = n;
  }
  const loaded = await page.locator(".topic-card").count();
  check("Infinite scroll loads every topic", all === 20 && firstPage < total && loaded === total, `first ${firstPage}, loaded ${loaded}/${total}`);

  // --- switching boards through the menu must not duplicate topics ---
  let libertyDup = 0;
  for (let i = 0; i < 2; i++) {
    for (const name of ["archive", "free"]) {
      await page.click(".liberty-navbar .dropdown-trigger >> text=게시판");
      await page.click(`.liberty-navbar a[href="/${name}"]`);
      await page.waitForURL(`${BBS}/${name}`);
      await page.waitForLoadState("networkidle");
      const ids = await page.locator("a.topic-card").evaluateAll((els) => els.map((e) => e.getAttribute("href")));
      libertyDup += ids.length - new Set(ids).size;
    }
  }
  check("Liberty: switching boards via the menu shows no duplicates", libertyDup === 0, `${libertyDup} duplicates`);

  // --- moderation menu (admin) ---
  const admin = await login(browser, "Admin", "mediabbs-dev-admin");
  await admin.page.goto(topicUrl, { waitUntil: "networkidle" });
  await admin.page.click(".admin-tools .dropdown-trigger button");
  await admin.page.click(".admin-tools >> text=상단 고정");
  await admin.page.waitForTimeout(500);
  const t1 = await api(admin.page, "GET", `/topics/${topicId}`);
  check("Admin menu pins the topic", t1.json.isPinned === true);
  await admin.page.click(".admin-tools .dropdown-trigger button");
  const unpin = await admin.page.waitForSelector(".admin-tools >> text=상단 고정 해제", { state: "visible", timeout: 5000 }).then(() => true, () => false);
  check("Admin menu now offers unpin", unpin);
  await admin.page.keyboard.press("Escape");

  // --- language switch ---
  await page.goto(BBS + "/free", { waitUntil: "networkidle" });
  await page.selectOption(".liberty-footer select[aria-label='언어']", "en");
  await page.waitForTimeout(300);
  check("Language switch updates the UI", (await page.textContent(".liberty-navbar")).includes("Recent changes") && (await page.getAttribute("html", "lang")) === "en");
  await page.reload({ waitUntil: "networkidle" });
  check("Language choice persists", (await page.textContent(".liberty-tools")).includes("New topic"));
  await page.selectOption(".liberty-footer select[aria-label='Language']", "ko");

  // --- skin switch ---
  await Promise.all([
    page.waitForEvent("load"),
    page.selectOption(".liberty-footer select[aria-label='스킨']", "simple"),
  ]);
  await page.waitForLoadState("networkidle");
  check("Skin switch loads the Simple skin", (await page.isVisible(".simple-app")) && !(await page.isVisible(".liberty-app")));
  const simpleMenu = await page.locator(".s-menu > .s-menu-item, .s-menu > .s-menu-dropdown > .s-menu-trigger").allInnerTexts();
  check("Simple menu from MENUS: boards dropdown + wiki", JSON.stringify(simpleMenu.map((x) => x.replace("▾", "").trim())) === JSON.stringify(["게시판", "위키"]), simpleMenu.join(","));
  await page.click(".s-menu-trigger");
  const boardLinks = await page.locator(".s-menu-panel a").allInnerTexts();
  check("Simple boards dropdown lists the boards", boardLinks.includes("자유게시판") && boardLinks.includes("보관함"), boardLinks.join(","));
  await page.click(".s-menu-panel >> text=보관함");
  await page.waitForURL(BBS + "/archive");
  check("Simple boards dropdown navigates", !(await page.isVisible(".s-menu-panel")));
  await page.click(".s-menu-trigger");
  await page.click('.s-menu-panel a[href="/free"]');
  await page.waitForURL(BBS + "/free");
  await page.waitForLoadState("networkidle");
  await page.waitForSelector("a.s-topic-title");
  const simpleIds = await page.locator("a.s-topic-title").evaluateAll((els) => els.map((e) => e.getAttribute("href")));
  check("Simple: switching boards via the menu shows no duplicates", simpleIds.length > 0 && simpleIds.length === new Set(simpleIds).size, `${simpleIds.length} shown`);
  const libertyCss = await page.evaluate(() => [...document.styleSheets].some((s) => [...(s.cssRules || [])].some((r) => r.selectorText === ".liberty-navbar")));
  check("No Liberty styles in the Simple skin", !libertyCss);
  await page.goto(topicUrl, { waitUntil: "networkidle" });
  await page.waitForSelector(".simple-topic .mb-editor");
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 800 });
    await page.waitForTimeout(300);
    const fits = await page.evaluate(() => {
      const box = document.querySelector(".simple-topic .s-compose").getBoundingClientRect();
      const ed = document.querySelector(".simple-topic .mb-editor").getBoundingClientRect();
      return ed.left >= box.left && ed.right <= box.right;
    });
    check(`Simple editor stays inside its box (${width}px)`, fits);
  }
  await page.setViewportSize({ width: 1280, height: 720 });
  const simpleBefore = await page.locator(".s-comment").count();
  await typeInEditor(page, "심플 스킨 의견");
  await page.click(".simple-topic .submit-button");
  await page.waitForFunction((n) => document.querySelectorAll(".s-comment").length === n + 1, simpleBefore, { timeout: 10000 });
  check("Simple skin posts a comment", true);
  await page.click(".s-comment >> nth=0 >> text=인용");
  await page.waitForTimeout(300);
  const simpleQuote = await page.locator(".simple-topic .mb-editor .ProseMirror").innerText();
  check("Simple skin quote fills the editor", simpleQuote.includes("첫 의견 본문"), simpleQuote.slice(0, 60));
  await Promise.all([page.waitForEvent("load"), page.selectOption(".s-footer select[aria-label='스킨']", "liberty")]);
  check("Skin switch back to Liberty", await page.isVisible(".liberty-app"));

  check("No uncaught page errors", pageErrors.length === 0, pageErrors.join(" | "));
  await browser.close();
  for (const r of results) console.log(r.join("  |  "));
  const failed = results.filter((r) => r[0] === "FAIL").length;
  console.log(failed ? `${failed} FAILED` : "ALL PASSED");
  process.exit(failed ? 1 : 0);
})().catch((e) => { for (const r of results) console.log(r.join("  |  ")); console.error(e); process.exit(1); });
