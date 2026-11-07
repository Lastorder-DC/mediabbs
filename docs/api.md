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
  }
}
```

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
  "createdAt": "…", "updatedAt": "…" }

// Topic
{ "id": 10, "boardId": 1, "title": "제목", "authorId": 3, "authorName": "Alice",
  "isClosed": false, "isSuspended": false, "isHidden": false, "isPinned": false,
  "commentCount": 4, "createdAt": "…", "updatedAt": "…" }

// Comment ("content"는 숨긴 의견이면 null)
{ "id": 20, "topicId": 10, "content": "본문(마크다운)", "authorId": 3, "authorName": "Alice",
  "isHidden": false, "createdAt": "…", "updatedAt": "…" }
```

비로그인 작성자는 `authorId`가 `null`이고 `authorName`이 IP 주소입니다.
주제의 첫 의견이 본문입니다.

| 메서드·경로 | 설명 | 응답 |
|---|---|---|
| `GET /boards` | 게시판 목록 (`isActive=false`는 보관됨) | `Board[]` |
| `GET /boards/{boardId}/topics?offset=&limit=` | 주제 목록. 상단 고정 → 최근 갱신 순, 숨긴 주제 제외. `limit` 기본 10, 최대 20 | `Topic[]`, `404` |
| `GET /topics/{topicId}` | 주제 하나 | `Topic`, `403 Topic is hidden`, `404` |
| `GET /topics/{topicId}/comments?offset=&limit=` | 의견 목록, 작성순. `limit` 기본 10, 최대 20 | `Comment[]` |
| `POST /topics` | 주제 작성 | `Topic` |
| `POST /topics/{topicId}/comments` | 의견 작성 | `204` |
| `GET /comments/{commentId}?showHidden=true` | 의견 하나. `showHidden`은 관리자만 | `Comment` |

`GET /topics/{id}`와 의견 목록은 `ETag`를 보내며 `If-None-Match`에 `304`로 답합니다.

### `POST /topics`

```json
{ "boardId": 1, "title": "제목 (1~100자)", "content": "첫 의견 본문, 마크다운 (1~100000자)" }
```

오류: `404 Board is not found`, `403 Board is not active`, `403 You are blocked`, `400` (검증 실패).

### `POST /topics/{topicId}/comments`

```json
{ "content": "본문 (1~100000자)" }
```

오류: `404`, `403 Topic is hidden` / `Topic is closed` / `Topic is suspended` /
`Board is not active` / `You are blocked`.

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

## 업로드

### `POST /files`

```json
{ "filename": "photo.png", "content": "<base64 인코딩한 파일 내용>" }
```

```json
{ "path": "bbs/V1StGXR8_Z5jdHi6B-myT.png", "url": "https://cdn.example.org/bbs/V1StGXR8_Z5jdHi6B-myT.png" }
```

오류: `404 Uploads are disabled`, `400`(허용되지 않는 형식, 내용이 확장자와 다름, 잘못된 base64, 빈 파일),
`413 File is too large`, `401 LoginRequired`, `403 You are blocked`.

### `GET /uploads/{filename}`

`STORAGE_BACKEND=local`일 때만 있습니다. 저장된 파일을 내보냅니다.
