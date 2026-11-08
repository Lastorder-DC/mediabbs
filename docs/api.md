# API

mediabbs-client가 쓰는 REST API입니다. 모든 엔드포인트는 `/v1` 접두사로도 접근할 수 있습니다
(`/boards`와 `/v1/boards`는 같음).

- 요청·응답 본문은 JSON이며 필드 이름은 **camelCase**입니다.
- 시각은 ISO 8601 UTC(`2026-11-06T12:57:25Z`)입니다.
- 오류 응답 본문은 짧은 영문 문자열입니다(예: `Topic is not found`).

## 인증

로그인 상태는 백엔드가 설정하는 HttpOnly 쿠키 `access_token`(MediaWiki가 발급한 JWT)과
`refresh_token`으로 유지합니다. 브라우저에서 호출할 때는 쿠키를 함께 보내야 합니다
(`fetch(..., { credentials: "include" })`, axios `withCredentials: true`).

| 응답 | 의미 | 클라이언트 동작 |
|---|---|---|
| `401 TokenExpired` | 액세스 토큰 만료 | `POST /auth/refresh` 후 같은 요청 재시도 |
| `401 TokenInvalid` | 토큰 위조·키 불일치·위키에서 철회됨 | 로그아웃 처리 |
| `401 TokenMissing` | 로그인 필요 (관리 기능 등) | 로그인 안내 |
| `403` | 권한 없음, 차단됨 등 (본문에 이유) | |

로그인 흐름:

1. 클라이언트가 브라우저를 `oauth.authorizeUrl?response_type=code&client_id=…&state=<돌아갈 경로>`로 보냅니다
   (`GET /config`에서 받은 값).
2. 위키가 로그인·승인 후 OAuth 콜백 URL(`<클라이언트>/auth?code=…&state=…`)로 돌려보냅니다.
3. 클라이언트가 `POST /auth/login`에 `code`를 보내면 백엔드가 위키와 토큰을 교환하고 쿠키를 설정합니다.

### `POST /auth/login`

```json
{ "code": "인가 코드" }
```

`204` + 쿠키 설정. 위키가 코드를 거부하면 `401 LoginFailed`.

### `POST /auth/refresh`

`refresh_token` 쿠키로 새 토큰을 받습니다. `204` + 쿠키 갱신, 실패 시 `401`.

### `POST /auth/logout`

쿠키를 지웁니다. `204`.

## 설정과 사용자

### `GET /config`

클라이언트용 공개 설정. 비밀 값은 포함하지 않습니다. 5분 캐시(위키에 접속하지 못했을 때는 30초).

```json
{
  "siteName": "예시 위키",
  "locale": "ko-KR",
  "defaultBoard": "free",
  "wiki": {
    "baseUrl": "https://wiki.example.org",
    "apiUrl": "https://wiki.example.org/w/api.php",
    "articlePath": "https://wiki.example.org/wiki/$1",
    "siteName": "예시 위키",
    "logo": "https://wiki.example.org/w/resources/assets/wiki.png",
    "lang": "ko"
  },
  "oauth": {
    "clientId": "0123…",
    "authorizeUrl": "https://wiki.example.org/w/rest.php/oauth2/authorize"
  },
  "navLinks": [],
  "recentChanges": {
    "enabled": true,
    "documentNamespaces": [0, 4, 10, 12, 14],
    "discussionNamespaces": [1, 3, 5, 7, 9, 11, 13, 15]
  },
  "siteNotice": { "enabled": true, "page": "MediaWiki:Sitenotice" },
  "upload": {
    "enabled": true,
    "requireLogin": true,
    "maxBytes": 10485760,
    "allowedExtensions": ["png", "jpg", "jpeg", "gif", "webp"]
  },
  "menus": null
}
```

`menus`는 관리 페이지에서 저장한 스킨별 메뉴입니다(없으면 `null`, 형식은 `PUT /admin/menus` 참고).
`siteName`과 `wiki.*` 중 위키에서 가져오는 값은 위키에 접속하지 못하면 `null`일 수 있습니다.
`locale`은 `SITE_LOCALE`을 설정하지 않았으면 `null`입니다.

### `GET /me`

로그인한 사용자 정보. 비로그인이면 `401 TokenMissing`. 60초 캐시(쿠키별).

```json
{
  "id": 3,
  "username": "Alice",
  "confirmedEmail": false,
  "blocked": false,
  "groups": ["*", "user", "autoconfirmed"],
  "rights": [],
  "email": "",
  "isAdmin": false
}
```

`email`은 컨슈머에 실명·이메일 권한이 없으면 빈 문자열입니다.

## 게시판과 주제

공통 객체:

```jsonc
// Board
{ "id": 1, "name": "free", "displayName": "자유게시판", "isActive": true,
  "sortOrder": 1, "icon": "chat-dots", "writePermission": "all", "commentPermission": "all",
  "isListed": true, "separatorBefore": false, "createdAt": "…", "updatedAt": "…" }

// Topic
{ "id": 10, "boardId": 1, "title": "제목", "authorId": 3, "authorName": "Alice",
  "isClosed": false, "isSuspended": false, "isHidden": false, "isPinned": false,
  "commentCount": 4, "createdAt": "…", "updatedAt": "…" }

// Comment ("content"는 숨긴 의견이면 null)
{ "id": 20, "topicId": 10, "content": "본문(마크다운)", "authorId": 3, "authorName": "Alice",
  "isHidden": false, "createdAt": "…", "updatedAt": "…" }
```

게시판 필드:

- `isActive`: `false`면 보관됨(읽기 전용).
- `icon`: 메뉴와 제목에서 이름 왼쪽에 보이는 [Bootstrap Icons](https://icons.getbootstrap.com/) 이름(`bi-` 없이). 없으면 `null`.
- `writePermission` / `commentPermission`: 새 주제 / 의견을 쓸 수 있는 사람. `all`(비로그인 포함 누구나),
  `users`(로그인 사용자), `admins`(게시판 관리자).
- `isListed`: 메뉴의 게시판 목록에 보일지. `false`여도 주소로는 들어갈 수 있습니다.
- `separatorBefore`: 메뉴의 게시판 목록에서 이 게시판 위에 구분선을 넣을지.
- `GET /boards`는 메뉴 순서(`sortOrder`)대로 줍니다.

비로그인 작성자는 `authorId`가 `null`이고 `authorName`이 IP 주소입니다.
주제의 첫 의견이 본문입니다.

| 메서드·경로 | 설명 | 응답 |
|---|---|---|
| `GET /boards` | 게시판 목록, 메뉴 순서대로 (`isListed=false` 포함) | `Board[]` |
| `GET /boards/{boardId}/topics?offset=&limit=&showHidden=` | 주제 목록. 상단 고정 → 최근 갱신 순, 숨긴 주제 제외(`showHidden=true`: 관리자만, 숨긴 주제 포함). `limit` 기본 10, 최대 20 | `Topic[]`, `404` |
| `GET /topics/{topicId}?showHidden=` | 주제 하나. 숨긴 주제는 `showHidden=true`(관리자만)일 때만 | `Topic`, `403 Topic is hidden`, `404` |
| `GET /topics/{topicId}/comments?offset=&limit=&showHidden=` | 의견 목록, 작성순. `limit` 기본 10, 최대 20. 숨긴 주제는 `showHidden=true`(관리자만)일 때만; 숨긴 의견의 내용은 여기서도 비어 있음 | `Comment[]`, `403 Topic is hidden` |
| `POST /topics` | 주제 작성 | `Topic` |
| `POST /topics/{topicId}/comments` | 의견 작성 | `204` |
| `GET /comments/{commentId}?showHidden=true` | 의견 하나. `showHidden`은 관리자만. 숨긴 주제의 의견은 `403 Topic is hidden` | `Comment` |

`GET /topics/{id}`와 의견 목록은 `ETag`를 보내며 `If-None-Match`에 `304`로 답합니다.

`showHidden=true`는 관리자가 숨긴 주제를 열어 숨김을 해제할 수 있게 하는 용도입니다. 관리자가 아니면 `401 TokenMissing` / `403 Admin only`이고, 이런 응답에는 `Cache-Control: private, no-store`가 붙습니다. 숨긴 주제는 실시간 갱신(`/events`)을 받지 않으므로 클라이언트는 폴링합니다.

### `POST /topics`

```json
{ "boardId": 1, "title": "제목 (1~100자)", "content": "첫 의견 본문, 마크다운 (1~100000자)" }
```

오류: `429 Too many posts`(아래), `404 Board is not found`, `403 Board is not active`, `403 You are blocked`, `400` (검증 실패),
`403 LoginRequired` / `403 Admin only` (게시판의 `writePermission`이 `users` / `admins`일 때).

주제·의견·업로드는 합쳐서 IP마다 1분에 `POSTS_PER_MINUTE`건(기본 10)까지 받습니다. 넘으면
`429 Too many posts`와 `Retry-After`(초)를 돌려줍니다.

### `POST /topics/{topicId}/comments`

```json
{ "content": "본문 (1~100000자)" }
```

오류: `429 Too many posts`, `404`, `403 Topic is hidden` / `Topic is closed` / `Topic is suspended` /
`Board is not active` / `You are blocked`, `403 LoginRequired` / `403 Admin only`
(게시판의 `commentPermission`이 `users` / `admins`일 때).

### `GET /topics/{topicId}/events`

주제 화면의 실시간 갱신(Server-Sent Events, `text/event-stream`). 로그인 필요 없음.
이벤트에는 무엇이 바뀌었는지만 들어 있고, 내용은 평소 API로 다시 받습니다(숨김 등 권한 규칙이 그대로 적용됨).

| 이벤트 | `data` | 의미 |
|---|---|---|
| `comment` | `{"id": 의견 ID}` | 새 의견 |
| `comment_updated` | `{"id": 의견 ID}` | 의견이 바뀜 (숨김/숨김 해제) |
| `topic` | `{"id": 주제 ID}` | 주제 상태가 바뀜 (종료, 잠금, 숨김, 고정) |
| `resync` | `{}` | 이벤트를 놓쳤으니 화면을 다시 불러올 것 |

연결하면 `retry: 5000`을 먼저 보내고, 25초마다 `: ping` 주석을 보냅니다.
오류: `404 Topic is not found`, `403 Topic is hidden`, `503 Live updates are unavailable`(꺼져 있거나, 동시 연결이
`LIVE_UPDATES_MAX_STREAMS`에 이르렀거나, 한 IP에서 16개를 넘게 열었을 때).

```js
const events = new EventSource("https://bbs-api.example.org/topics/12/events");
events.addEventListener("comment", (e) => console.log("새 의견", JSON.parse(e.data).id));
```

### `GET /search`

주제 제목과 의견 내용 검색. 로그인 필요 없음.

| 매개변수 | 설명 |
|---|---|
| `q` | 검색어 (1~100자). 띄어 쓴 단어(최대 5개)를 **모두** 포함해야 일치합니다. 대소문자 무시 |
| `in` | `all`(기본, 제목 또는 의견 내용), `title`, `content`(의견 내용), `author`(작성자 이름 또는 비로그인 작성자의 IP와 정확히 일치) |
| `board` | 이 게시판 ID만 |
| `offset`, `limit` | 페이지 (`limit` 기본 20, 최대 50) |

결과는 주제 단위로, 최근 활동(`updatedAt`) 순입니다. 숨긴 주제와 숨긴 의견은 나오지 않습니다.

```json
{
  "results": [
    {
      "topic": { "id": 12, "boardId": 1, "title": "사과 이야기", "...": "Topic의 나머지 필드" },
      "boardName": "free", "boardDisplayName": "자유게시판",
      "titleMatch": true,
      "commentMatches": 5,
      "comments": [
        { "id": 40, "authorName": "Alice", "createdAt": "2026-11-07T08:00:00Z", "snippet": "…빨간 사과는 맛있다…" }
      ]
    }
  ],
  "hasMore": false
}
```

`comments`에는 일치한 의견 중 앞의 3개가 들어가고, `snippet`은 첫 일치 부분 주변의 일반 텍스트(Markdown 기호 제거)입니다.
오류: `400 Empty search`, `400 Search is too long`, `400` (모르는 `in` 값),
`503 Search is busy`(동시에 3개까지만 실행, 잠시 뒤 다시). `offset`은 1000까지입니다.

검색은 `LIKE '%단어%'`로 표를 훑습니다. MariaDB의 FULLTEXT는 띄어쓰기로만 단어를 나눠 한국어에 맞지 않기
때문입니다. 게시판 규모(수십만 의견 정도)에서는 충분하지만, 아주 큰 DB에서는 느려질 수 있습니다.

## 관리 (관리자만)

관리자가 아니면 `403`, 비로그인이면 `401 TokenMissing`.

### `PUT /topics/{topicId}/status`

바꿀 항목만 보냅니다. 한 번에 하나씩 보내는 것을 권장합니다(관리 기록은 요청당 하나).

```json
{ "isHidden": true }        // isClosed, isSuspended, isHidden, isPinned
```

응답: 바뀐 `Topic`.

### `PUT /comments/{commentId}/status`

```json
{ "isHidden": true }
```

응답: 바뀐 `Comment` (관리자에게는 숨긴 내용도 포함).

### 게시판 관리

클라이언트의 관리 페이지(`/admin`)가 쓰는 API입니다. 바꾼 내용은 모두 `logs`에 기록됩니다.
관리자가 아니면 `403 Admin only`.

| 메서드·경로 | 설명 | 응답 |
|---|---|---|
| `POST /admin/boards` | 게시판 만들기 (메뉴 맨 끝에 추가) | `Board`, `409 Board name is taken` |
| `PUT /admin/boards/{boardId}` | 게시판 설정 바꾸기 | `Board`, `404`, `409 Board name is taken` |
| `DELETE /admin/boards/{boardId}` | 주제가 없는 게시판 삭제 | `204`, `404`, `409 Board has topics` |
| `PUT /admin/boards-order` | 메뉴 순서 바꾸기 | `Board[]`, `400 Every board must be listed once` |
| `PUT /admin/menus` | 스킨별 메뉴 저장 | `{ "menus": … }`, `400 Invalid menu: …` |
| `GET /admin/logs` | 관리 기록 (최신순) | 아래 참고, `400 Invalid log type` |

게시판 만들기·바꾸기 (모든 필드 필요):

```json
{
  "name": "notice", "displayName": "공지사항", "icon": "megaphone",
  "writePermission": "admins", "commentPermission": "users",
  "isListed": true, "isActive": true, "separatorBefore": false
}
```

`name`은 `mediabbs board add`와 같은 규칙, `icon`은 Bootstrap Icons 이름(`bi-`를 붙여도 됨) 또는 `null`.
검증 실패는 `400 Invalid board name` / `Invalid display name` / `Invalid icon name`.

관리 기록은 `?limit=50&before=<id>&types=<종류>,<종류>`로 조회합니다.
`limit`는 1–200(기본 50), `before`는 이전 페이지의 마지막 `id`(더 오래된 기록부터), `types`는 쉼표로 구분한
종류 이름입니다: `close_topic`, `unclose_topic`, `hide_topic`, `unhide_topic`, `suspend_topic`,
`unsuspend_topic`, `pin_topic`, `unpin_topic`, `hide_comment`, `unhide_comment`, `create_board`,
`update_board`, `delete_board`, `reorder_boards`, `update_menus`. IP 주소는 내보내지 않습니다.

```json
{
  "logs": [
    {
      "id": 42, "type": "hide_comment", "target": 410,
      "userId": 1, "userName": "Admin", "createdAt": "2026-11-07T08:17:24Z",
      "topicId": 368, "topicTitle": "Alice의 글",
      "boardName": "free", "boardDisplayName": "자유게시판"
    }
  ],
  "hasMore": true
}
```

`target`은 주제·의견·게시판 ID(대상이 없으면 0)입니다. `topicId`/`topicTitle`은 주제와 의견 기록에서,
`boardName`/`boardDisplayName`은 주제가 속한 게시판이나 게시판 기록의 게시판입니다(지워졌으면 `null`).

순서 바꾸기는 모든 게시판 ID를 원하는 순서로 보냅니다.

```json
{ "ids": [3, 1, 2] }
```

메뉴 저장은 스킨 이름(또는 모든 스킨에 쓰는 `default`)별 메뉴 항목 배열입니다. 항목 형식은 클라이언트
`config.js`의 `menus`와 같습니다([mediabbs-client README](https://github.com/Lastorder-DC/mediabbs-client#상단-메뉴)).
`null`을 보내면 저장된 메뉴를 지웁니다(클라이언트 `config.js` 또는 스킨 기본 메뉴로 돌아감).
링크는 `http(s)://` 주소나 `/`로 시작하는 경로만 쓸 수 있습니다.

```json
{
  "menus": {
    "liberty": [
      { "type": "boards" },
      { "type": "recentChanges" },
      { "label": { "ko": "도움말", "en": "Help" }, "url": "https://wiki.example.org/wiki/Help:Contents" },
      { "label": "더 보기", "children": [{ "label": "규칙", "url": "/notice/1" }] }
    ]
  }
}
```

## 업로드

### `POST /files`

```json
{ "filename": "photo.png", "content": "<base64 인코딩한 파일 내용>" }
```

```json
{ "path": "bbs/V1StGXR8_Z5jdHi6B-myT.png", "url": "https://cdn.example.org/bbs/V1StGXR8_Z5jdHi6B-myT.png" }
```

`Content-Type: application/json`이어야 합니다(`415 Expected JSON`). 로그인·차단 확인은 본문을 읽기 전에 합니다.
오류: `404 Uploads are disabled`, `400`(허용되지 않는 형식, 내용이 확장자와 다름, 잘못된 base64, 빈 파일),
`413 File is too large`, `401 LoginRequired`, `403 You are blocked`, `429 Too many posts`.

### `GET /uploads/{filename}`

`STORAGE_BACKEND=local`일 때만 있습니다. 저장된 파일을 내보냅니다.
