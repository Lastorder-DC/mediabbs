// Browser test of topic/comment search on the dev stack (docker-compose.yml).
//   node search-e2e.js
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

(async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ locale: "ko-KR" });
  const page = await ctx.newPage();
  const pageErrors = [];
  page.on("pageerror", (e) => pageErrors.push(String(e)));
  await page.goto(BBS + "/free", { waitUntil: "networkidle" });

  // Unique words so earlier runs do not match.
  const stamp = Date.now().toString(36);
  const titleWord = `제목어${stamp}`, bodyWord = `본문어${stamp}`;
  const topic = await api(page, "POST", "/topics", { boardId: 1, title: `검색 테스트 ${titleWord}`, content: "첫 의견" });
  check("Test topic posted", topic.status === 200, String(topic.status));
  const topicId = topic.json.id;
  await api(page, "POST", `/topics/${topicId}/comments`, { content: `앞부분 **${bodyWord}** 뒷부분 100%` });
  const q = (params) => api(page, "GET", "/search?" + new URLSearchParams(params));

  // --- API ---
  let r = await q({ q: titleWord });
  check("Search finds a topic by its title", r.status === 200 && r.json.results.length === 1 && r.json.results[0].topic.id === topicId && r.json.results[0].titleMatch && r.json.results[0].boardName === "free", r.text.slice(0, 200));
  r = await q({ q: bodyWord });
  const hit = r.json.results[0];
  check("Search finds a topic by a comment", r.json.results.length === 1 && hit.topic.id === topicId && !hit.titleMatch && hit.commentMatches === 1, r.text.slice(0, 200));
  check("Comment snippet is plain text around the match", hit.comments[0].snippet === `앞부분 ${bodyWord} 뒷부분 100%`, hit.comments[0].snippet);
  check("Every word must match", (await q({ q: `${bodyWord} 없는말${stamp}` })).json.results.length === 0);
  check("Words can match in any order", (await q({ q: `뒷부분 ${bodyWord}` })).json.results.length === 1);
  check("Title-only search skips comments", (await q({ q: bodyWord, in: "title" })).json.results.length === 0);
  check("Comment-only search skips titles", (await q({ q: titleWord, in: "content" })).json.results.length === 0);
  check("Board filter", (await q({ q: titleWord, board: "2" })).json.results.length === 0 && (await q({ q: titleWord, board: "1" })).json.results.length === 1);
  check("LIKE wildcards are matched literally", (await q({ q: `${bodyWord.slice(0, -1)}_` })).json.results.length === 0);
  const ip = topic.json.authorName;
  const byIp = await q({ q: ip, in: "author", limit: "50" });
  check("Author search matches an anonymous author's IP", byIp.json.results.some((h) => h.topic.id === topicId), ip);
  check("Empty search is refused", (await q({ q: "  " })).status === 400);
  check("Overlong search is refused", (await q({ q: "가".repeat(101) })).status === 400);
  check("Unknown search field is refused", (await q({ q: "x", in: "nope" })).status === 400);

  // --- Liberty: board search box -> search page ---
  await page.fill(".liberty-tools .board-search input", bodyWord);
  await page.press(".liberty-tools .board-search input", "Enter");
  await page.waitForURL(/\/search\?/);
  await page.waitForSelector(".mb-search-hit");
  check("Liberty: board search opens /search for the board", page.url().includes("board=1") && page.url().includes(encodeURIComponent(bodyWord)), page.url());
  check("Search page highlights the match in the snippet", (await page.locator(".mb-search-snippet mark").first().innerText()) === bodyWord);
  check("Search page keeps the board selected", (await page.inputValue("select[name=board]")) === "1");
  await page.click(".mb-search-topic");
  await page.waitForURL(`${BBS}/free/${topicId}`);
  check("Result links to the topic", true);
  await page.goBack();
  await page.waitForSelector(".mb-search-hit");
  check("Back returns to the results", (await page.inputValue("input[name=q]")) === bodyWord);
  await page.selectOption("select[name=in]", "title");
  await page.click(".mb-search-button");
  await page.waitForSelector(".mb-search-empty");
  check("Changing the field searches again (no title match)", page.url().includes("in=title") && (await page.textContent(".mb-search-empty")).includes(bodyWord));

  // --- Simple ---
  await ctx.addInitScript(() => localStorage.setItem("mediabbs.skin", "simple"));
  await page.goto(BBS + "/free", { waitUntil: "networkidle" });
  await page.fill(".s-title-row .board-search input", titleWord);
  await page.click(".s-title-row .board-search button");
  await page.waitForSelector(".mb-search-hit");
  check("Simple: board search shows the result", (await page.locator(".mb-search-topic mark").innerText()) === titleWord);
  check("Simple: no board search box on the search page", (await page.locator(".s-title-row .board-search").count()) === 0);

  check("No uncaught page errors", pageErrors.length === 0, pageErrors.join(" | "));
  await browser.close();
  for (const r of results) console.log(r.join("  |  "));
  const failed = results.filter((r) => r[0] === "FAIL").length;
  console.log(failed ? `${failed} FAILED` : "ALL PASSED");
  process.exit(failed ? 1 : 0);
})().catch((e) => { for (const r of results) console.log(r.join("  |  ")); console.error(e); process.exit(1); });
