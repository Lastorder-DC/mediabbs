// Browser test of board administration on the dev stack (docker-compose.yml):
// the admin page (boards: settings, icons, separators, order, visibility,
// write permissions; menus per skin) and how both skins show the result.
//   node admin-e2e.js        (after oauth-e2e.js's setup steps; see README.md)
const { chromium } = require("playwright");
const BBS = "http://127.0.0.1:18090", API = "http://127.0.0.1:18081";
const results = [];
const check = (name, ok, detail = "") => results.push([ok ? "PASS" : "FAIL", name, detail]);

async function api(page, method, path, body) {
  return page.evaluate(async ([API, method, path, body]) => {
    const r = await fetch(API + path, {
      method, credentials: "include", cache: "reload",
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

/** A fresh anonymous visitor (empty cache), optionally with a skin. */
async function visitor(browser, skin) {
  const ctx = await browser.newContext({ locale: "ko-KR" });
  if (skin) await ctx.addInitScript((s) => localStorage.setItem("mediabbs.skin", s), skin);
  const page = await ctx.newPage();
  return { ctx, page };
}

const libertyBoardsMenu = async (page) => {
  await page.click(".liberty-navbar .dropdown-trigger >> text=게시판");
  await page.waitForSelector(".liberty-navbar .dropdown.is-active .dropdown-content");
  return page.locator(".liberty-navbar .dropdown.is-active .dropdown-content");
};

(async () => {
  const browser = await chromium.launch();
  const pageErrors = [];
  const name = `e2e-notice-${Date.now().toString(36)}`;

  // --- not for anonymous users ---
  const anon = await visitor(browser);
  anon.page.on("pageerror", (e) => pageErrors.push(String(e)));
  await anon.page.goto(BBS + "/admin", { waitUntil: "networkidle" });
  check("Anonymous: admin page asks for an admin login", (await anon.page.textContent(".mb-admin")).includes("로그인한 게시판 관리자만"));
  const anonCreate = await api(anon.page, "POST", "/admin/boards", { name: "anon-board", displayName: "x", icon: null, writePermission: "all", commentPermission: "all", isListed: true, isActive: true, separatorBefore: false });
  check("Anonymous: admin API refuses", anonCreate.status === 401, String(anonCreate.status));

  // --- admin: create a board from the admin page ---
  const admin = await login(browser, "Admin", "mediabbs-dev-admin");
  const page = admin.page;
  page.on("pageerror", (e) => pageErrors.push(String(e)));
  page.on("dialog", (d) => d.accept());
  await page.click(".user-items .user-gravatar-wrapper");
  await page.click(".liberty-navbar >> text=게시판 관리");
  await page.waitForURL(BBS + "/admin");
  await page.waitForSelector(".mb-board-row");
  check("Admin page opens from the user menu", await page.isVisible(".mb-tabs"));
  await page.click(".new-board");
  await page.fill(".mb-board-form input[name=displayName]", "공지 E2E");
  await page.fill(".mb-board-form input[name=name]", name);
  await page.fill(".mb-board-form input[name=icon]", "bi-megaphone");
  await page.selectOption(".mb-board-form select[name=writePermission]", "admins");
  await page.selectOption(".mb-board-form select[name=commentPermission]", "users");
  await page.check(".mb-board-form input[name=separatorBefore]");
  await page.click(".save-board");
  await page.waitForSelector(`.mb-board-row[data-board="${name}"]`);
  const row = page.locator(`.mb-board-row[data-board="${name}"]`);
  check("New board is listed with its icon", await row.locator("i.bi-megaphone").count() === 1);
  let boards = (await api(page, "GET", "/boards")).json;
  let created = boards.find((b) => b.name === name);
  check("Board settings are saved", created && created.icon === "megaphone" && created.writePermission === "admins" && created.commentPermission === "users" && created.separatorBefore && created.isListed && boards[boards.length - 1].name === name, JSON.stringify(created));

  // --- the menu shows the icon and the separator (both skins) ---
  let menu = await libertyBoardsMenu(page);
  const libertyItems = await menu.evaluate((el) => [...el.children].map((c) => (c.tagName === "HR" ? "---" : c.textContent.trim())));
  check("Liberty menu: separator above the board", libertyItems[libertyItems.length - 2] === "---" && libertyItems[libertyItems.length - 1] === "공지 E2E", libertyItems.join(" | "));
  check("Liberty menu: board icon", await menu.locator(`a[href="/${name}"] i.bi-megaphone`).count() === 1);
  await page.keyboard.press("Escape");
  const simple = await visitor(browser, "simple");
  simple.page.on("pageerror", (e) => pageErrors.push(String(e)));
  await simple.page.goto(BBS + "/free", { waitUntil: "networkidle" });
  await simple.page.click(".s-menu-trigger >> text=게시판");
  const simpleItems = await simple.page.locator(".s-menu-panel").evaluate((el) => [...el.children].map((c) => (c.tagName === "HR" ? "---" : c.textContent.trim())));
  check("Simple menu: separator above the board", simpleItems[simpleItems.length - 2] === "---" && simpleItems[simpleItems.length - 1] === "공지 E2E", simpleItems.join(" | "));
  check("Simple menu: board icon", await simple.page.locator(`.s-menu-panel a[href="/${name}"] i.bi-megaphone`).count() === 1);
  await simple.page.click(`.s-menu-panel a[href="/${name}"]`);
  await simple.page.waitForURL(BBS + "/" + name);
  check("Simple: board title shows the icon", await simple.page.locator(".s-title-row h1 i.bi-megaphone").count() === 1);

  // --- write permissions ---
  check("Admins-only board: no write button for visitors", !(await simple.page.isVisible(".s-actions")));
  const anonTopic = await api(anon.page, "POST", "/topics", { boardId: created.id, title: "x", content: "x" });
  check("Admins-only board: API refuses a visitor's topic", anonTopic.status === 403 && anonTopic.text === "Admin only", `${anonTopic.status} ${anonTopic.text}`);
  const adminTopic = await api(page, "POST", "/topics", { boardId: created.id, title: "공지 테스트", content: "본문" });
  check("Admins-only board: admin can post", adminTopic.status === 200, String(adminTopic.status));
  const anonComment = await api(anon.page, "POST", `/topics/${adminTopic.json.id}/comments`, { content: "x" });
  check("Users-only comments: API refuses a visitor", anonComment.status === 403 && anonComment.text === "LoginRequired", `${anonComment.status} ${anonComment.text}`);
  const adminComment = await api(page, "POST", `/topics/${adminTopic.json.id}/comments`, { content: "관리자 의견" });
  check("Users-only comments: logged-in user can comment", adminComment.status === 204, String(adminComment.status));
  await simple.page.goto(`${BBS}/${name}/${adminTopic.json.id}`, { waitUntil: "networkidle" });
  check("Users-only comments: visitors see why they cannot comment", (await simple.page.textContent(".simple-topic")).includes("로그인해야 이 게시판에 의견을 쓸 수 있습니다"));
  await page.goto(`${BBS}/${name}`, { waitUntil: "networkidle" });
  check("Admins-only board: admin sees the write button", await page.isVisible(".liberty-tools .write-button"));

  // --- order ---
  await page.goto(BBS + "/admin", { waitUntil: "networkidle" });
  await page.waitForSelector(`.mb-board-row[data-board="${name}"]`);
  const count = await page.locator(".mb-board-row").count();
  for (let i = 0; i < count - 1; i++) {
    await page.locator(`.mb-board-row[data-board="${name}"] button[aria-label="위로"]`).click();
  }
  check("Moving shows the save-order bar", await page.isVisible(".save-order"));
  await page.click(".save-order");
  await page.waitForSelector(".save-order", { state: "detached" });
  boards = (await api(page, "GET", "/boards")).json;
  check("Order is saved", boards[0].name === name, boards.map((b) => b.name).join(","));
  menu = await libertyBoardsMenu(page);
  const firstItems = await menu.evaluate((el) => [...el.children].slice(0, 2).map((c) => (c.tagName === "HR" ? "---" : c.textContent.trim())));
  check("Menu follows the order (no separator above the first board)", firstItems[0] === "공지 E2E" && firstItems[1] !== "---", firstItems.join(" | "));
  await page.keyboard.press("Escape");

  // --- hide from the menu ---
  await page.locator(`.mb-board-row[data-board="${name}"] .edit-board`).click();
  await page.uncheck(".mb-board-form input[name=isListed]");
  await page.click(".save-board");
  await page.waitForSelector(`.mb-board-row[data-board="${name}"] .mb-badge`);
  menu = await libertyBoardsMenu(page);
  check("Hidden board is left out of the menu", await menu.locator(`a[href="/${name}"]`).count() === 0);
  await page.keyboard.press("Escape");
  const fresh = await visitor(browser);
  await fresh.page.goto(BBS + "/" + name, { waitUntil: "networkidle" });
  check("Hidden board is still reachable by URL", (await fresh.page.textContent(".title-wrapper-row")).includes("공지 E2E"));
  await fresh.page.goto(BBS + "/", { waitUntil: "networkidle" });
  check("Home skips the hidden first board", !fresh.page.url().endsWith("/" + name), fresh.page.url());
  await fresh.ctx.close();

  // --- delete (only boards without topics) ---
  const tmp = await api(page, "POST", "/admin/boards", { name: `${name}-tmp`, displayName: "임시", icon: null, writePermission: "all", commentPermission: "all", isListed: true, isActive: true, separatorBefore: false });
  check("Admin API creates a board", tmp.status === 200, tmp.text.slice(0, 100));
  const dup = await api(page, "POST", "/admin/boards", { name: `${name}-tmp`, displayName: "x", icon: null, writePermission: "all", commentPermission: "all", isListed: true, isActive: true, separatorBefore: false });
  check("Duplicate board name is refused", dup.status === 409, String(dup.status));
  const badIcon = await api(page, "PUT", `/admin/boards/${tmp.json.id}`, { ...tmp.json, icon: "x\"><b>", id: undefined, sortOrder: undefined, createdAt: undefined, updatedAt: undefined });
  check("Invalid icon name is refused", badIcon.status === 400, `${badIcon.status} ${badIcon.text}`);
  const hasTopics = await api(page, "DELETE", `/admin/boards/${created.id}`);
  check("A board with topics cannot be deleted", hasTopics.status === 409, String(hasTopics.status));
  await page.reload({ waitUntil: "networkidle" });
  await page.locator(`.mb-board-row[data-board="${name}-tmp"] .edit-board`).click();
  await page.click(".delete-board");
  await page.waitForSelector(`.mb-board-row[data-board="${name}-tmp"]`, { state: "detached" });
  check("Admin page deletes an empty board", !(await api(page, "GET", "/boards")).json.some((b) => b.name === `${name}-tmp`));

  // --- menus ---
  await page.click(".mb-tab >> text=메뉴");
  await page.selectOption("select[name=menuSkin]", "liberty");
  check("Menu editor shows where the menu comes from", (await page.textContent(".mb-admin-hint")).includes("스킨 기본 메뉴"));
  const items = page.locator(".mb-menu-item");
  // Liberty default: recentChanges, random, boards, tools, navLinks
  await items.nth(1).locator('button[aria-label="빼기"]').click(); // random
  await items.nth(1).locator('button[aria-label="위로"]').click(); // boards to the top
  await page.click(".add-link");
  const link = items.last();
  await link.locator("input[name=label]").fill("도움말");
  await link.locator("input[name=url]").fill("https://example.org/help");
  await page.click(".save-menus");
  await page.waitForSelector(".mb-admin-message:not(.error)");
  const config = (await api(page, "GET", "/config")).json;
  check("Menu is saved for the skin", JSON.stringify(config.menus?.liberty?.map((i) => i.type ?? i.label)) === JSON.stringify(["boards", "recentChanges", "tools", "navLinks", "도움말"]), JSON.stringify(config.menus));
  const v = await visitor(browser);
  await v.page.goto(BBS + "/free", { waitUntil: "networkidle" });
  const navbar = (await v.page.locator(".liberty-navbar .navbar-start .navbar-text").allInnerTexts()).slice(1);
  check("Visitors get the saved menu", JSON.stringify(navbar) === JSON.stringify(["게시판", "최근 바뀜", "도구", "도움말"]), navbar.join(","));
  check("Saved link points to its URL", (await v.page.getAttribute(".liberty-navbar a.navbar-item[title='도움말']", "href")) === "https://example.org/help");
  await v.ctx.close();
  const vs = await visitor(browser, "simple");
  await vs.page.goto(BBS + "/free", { waitUntil: "networkidle" });
  const simpleMenu = await vs.page.locator(".s-menu > .s-menu-item, .s-menu > .s-menu-dropdown > .s-menu-trigger").allInnerTexts();
  check("Other skins keep their own menu (MENUS)", JSON.stringify(simpleMenu.map((x) => x.replace("▾", "").trim())) === JSON.stringify(["게시판", "위키"]), simpleMenu.join(","));
  await vs.ctx.close();
  const evil = await api(page, "PUT", "/admin/menus", { menus: { liberty: [{ label: "x", url: "javascript:alert(1)" }] } });
  check("Unsafe menu links are refused", evil.status === 400, `${evil.status} ${evil.text}`);
  const anonMenus = await api(anon.page, "PUT", "/admin/menus", { menus: null });
  check("Anonymous: menus cannot be changed", anonMenus.status === 401, String(anonMenus.status));

  // JSON editor round trip, then remove the saved menu again
  await page.click(".toggle-json");
  const json = JSON.parse(await page.inputValue("textarea[name=menusJson]"));
  check("JSON editor shows the saved menus", Array.isArray(json.liberty) && json.liberty.length === 5);
  await page.click(".toggle-json");
  await page.click(".reset-menu");
  await page.click(".save-menus");
  await page.waitForSelector(".mb-admin-message:not(.error)");
  check("Removing the saved menu restores the default", (await api(page, "GET", "/config")).json.menus === null);

  // Leave the test board out of the way for other tests: last, hidden.
  boards = (await api(page, "GET", "/boards")).json;
  const ids = boards.map((b) => b.id).filter((id) => id !== created.id).concat(created.id);
  await api(page, "PUT", "/admin/boards-order", { ids });

  check("No uncaught page errors", pageErrors.length === 0, pageErrors.join(" | "));
  await browser.close();
  for (const r of results) console.log(r.join("  |  "));
  const failed = results.filter((r) => r[0] === "FAIL").length;
  console.log(failed ? `${failed} FAILED` : "ALL PASSED");
  process.exit(failed ? 1 : 0);
})().catch((e) => { for (const r of results) console.log(r.join("  |  ")); console.error(e); process.exit(1); });
