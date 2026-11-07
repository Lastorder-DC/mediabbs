# 문제 해결

먼저 백엔드 로그를 보세요(`journalctl -u mediabbs -f` 또는 `docker logs -f mediabbs`).
`RUST_LOG=actix_web=info,mediabbs=debug`로 더 자세한 로그를 볼 수 있습니다.

## 백엔드가 시작하지 않음

| 메시지 | 원인과 해결 |
|---|---|
| `Configuration error: XXX is not set` | 필수 설정이 빠졌습니다. `.env`가 **작업 디렉터리**에 있는지, Docker라면 `--env-file`을 넘겼는지 확인 |
| `Configuration error: cannot read OAUTH_PUBLIC_KEY_PATH` | 공개키 파일 경로가 틀렸거나 읽을 권한이 없습니다. 상대 경로는 작업 디렉터리 기준 |
| `Configuration error: invalid public key` | PEM 형식이 아닙니다. `-----BEGIN PUBLIC KEY-----`로 시작하는 위키의 공개키를 넣으세요 (개인키 아님) |
| `Configuration error: NAV_LINKS is not valid JSON` | `.env`에서 JSON을 작은따옴표로 감싸지 않으면 큰따옴표가 사라집니다: `NAV_LINKS='[...]'` |
| `Configuration error: MW_ARTICLE_PATH must contain $1` | `.env`에서 `$1`이 치환된 것입니다. `MW_ARTICLE_PATH='/wiki/$1'` |
| `Configuration error: S3_BUCKET is not set (or set UPLOAD_ENABLED=false)` | 업로드는 기본으로 켜져 있습니다. 저장소를 설정하거나 `UPLOAD_ENABLED=false` |
| `Failed to create pool` / `cannot connect to database` | `DATABASE_URL` 확인. Docker 컨테이너 안의 `localhost`는 호스트가 아닙니다 |
| `Address already in use` | `BIND_ADDRESS` 포트를 다른 프로그램이 쓰고 있습니다 |

## 화면에 "서버(…)에서 설정을 불러오지 못했습니다"

클라이언트가 `GET <백엔드>/config`를 받지 못했습니다.

1. 메시지 괄호 안의 백엔드 주소가 맞는지 확인 → 클라이언트의 `config.js`(또는 `BACKEND_HOST`).
2. 그 주소 + `/config`를 브라우저에서 직접 열어 JSON이 나오는지 확인.
3. 브라우저 개발자 도구 콘솔에 **CORS 오류**가 있다면 백엔드의 `CORS_ALLOWED_ORIGINS`에 클라이언트 주소를
   **정확히**(스킴·포트 포함, 끝의 `/` 없이) 넣으세요. 예: `https://bbs.example.org`
4. 백엔드는 HTTPS, 클라이언트는 HTTP(또는 그 반대)이면 혼합 콘텐츠로 막힙니다.

## 로그인

| 증상 | 원인과 해결 |
|---|---|
| 위키에서 "OAuth 콜백 URL이 일치하지 않음" 류의 오류 | 컨슈머의 콜백 URL이 `<클라이언트 주소>/auth`와 정확히 같은지 확인 |
| 위키에서 "알 수 없는 컨슈머" / "승인되지 않음" | `Special:OAuthManageConsumers`에서 컨슈머를 승인했는지, `OAUTH_CLIENT_ID`가 맞는지 확인 |
| 승인 후 "로그인 중 에러가 발생했습니다" (`401 LoginFailed`) | 토큰 교환 실패. `OAUTH_CLIENT_SECRET` 확인. 백엔드 로그의 `token request rejected by wiki` 상태 코드 참고. 컨슈머가 **confidential**이고 승인 유형에 Authorization code가 있는지 확인 |
| 승인 후 같은 메시지 + 백엔드 로그에 `token request failed` | 백엔드가 위키 `rest.php`에 접속하지 못합니다. 방화벽·DNS 확인, 필요하면 `MW_REST_URL`로 내부 주소 지정 |
| 로그인 직후에도 로그인 버튼이 그대로 | 쿠키가 저장·전송되지 않습니다. HTTP로 운영 중이면 `COOKIE_SECURE=false`. 클라이언트와 백엔드가 다른 사이트면 `COOKIE_SAMESITE=None` (+HTTPS). 개발자 도구 → 애플리케이션 → 쿠키에서 `access_token` 확인 |
| 모든 요청이 `401 TokenInvalid` | 백엔드의 공개키가 위키의 `$wgOAuth2PublicKey`와 다르거나(키를 바꿨다면 백엔드도 교체 후 재시작), `OAUTH_CLIENT_ID`가 토큰을 발급한 컨슈머와 다름 |
| 1시간쯤 지나면 로그아웃됨 | 정상이라면 자동 갱신됩니다. 리프레시 토큰이 만료됐거나(위키 `$wgOAuth2RefreshTokenTTL`) 위키의 세션 캐시(`$wgMWOAuthSessionCacheType`)가 재시작 때마다 비워지는지 확인 |

## 권한·차단

| 증상 | 원인과 해결 |
|---|---|
| 관리자 메뉴가 안 보임 | `GET /me`의 `groups`에 `ADMIN_GROUPS`의 그룹이 있는지 확인. `ADMIN_RIGHTS`는 신원 확인용 컨슈머에서 동작하지 않음. 그룹을 방금 바꿨다면 1분 뒤 새로 고침 |
| 차단을 풀었는데 화면에 "차단되어 있어…" | 화면 정보는 최대 1분 캐시됩니다. 새로 고침. 서버는 매 요청 위키에 확인하므로 글쓰기는 즉시 가능 |
| 비로그인 사용자가 모두 같은 IP로 표시 | 백엔드가 프록시 주소를 사용자 IP로 보고 있습니다. nginx의 `X-Forwarded-For` 설정과 `TRUST_PROXY` 확인 ([설치 안내 6장](installation.md#실제-사용자-ip)) |
| 위키에서 IP를 차단했는데 비로그인 글쓰기가 됨 | 위와 같은 원인(백엔드가 실제 IP를 모름)이거나 `CHECK_IP_BLOCKS=false` |

## 위키 정보·링크

| 증상 | 원인과 해결 |
|---|---|
| 위키 이름이 "Wiki"로, 로고가 기본 아이콘으로 나옴 | 백엔드가 위키 `api.php`에 접속하지 못합니다(`GET /config`의 `wiki.siteName`이 `null`). `MW_BASE_URL`, `MW_SCRIPT_PATH`, 필요하면 `MW_API_URL` 확인. 로고만 기본 아이콘이면 위키의 `$wgLogo` 주소가 깨진 것 |
| 위키 링크가 404 | `MW_SCRIPT_PATH`가 틀렸거나 문서 주소 형식이 다릅니다. `MW_ARTICLE_PATH='/wiki/$1'`처럼 직접 지정 |
| 최근 바뀜/사이트 공지가 안 나옴 | 브라우저가 위키 `api.php`를 직접 읽습니다. 콘솔 오류 확인. 위키 앞단(CDN, WAF)이 `origin=*` 요청을 막고 있지 않은지 확인. 공지 문서가 없으면 표시하지 않습니다 |

## 업로드

| 증상 | 원인과 해결 |
|---|---|
| "파일을 업로드하려면 로그인해야 합니다" | `UPLOAD_REQUIRE_LOGIN=true`(기본). 비로그인 업로드를 허용하려면 `false` |
| "업로드할 수 있는 파일 형식" | `UPLOAD_ALLOWED_EXTENSIONS`에 없는 확장자 |
| `400 File content does not match its extension` | 확장자와 실제 형식이 다름(예: 이름만 `.png`인 JPEG). 파일을 다시 저장하거나 올바른 확장자로 |
| `413` / nginx `413 Request Entity Too Large` | `UPLOAD_MAX_BYTES` 초과, 또는 nginx `client_max_body_size`가 작음(업로드 크기의 약 4/3 이상 필요) |
| 업로드는 되는데 이미지가 깨짐 | `UPLOAD_PUBLIC_URL`이 실제로 파일을 내보내는 주소가 아님. 반환된 `url`을 직접 열어 보고, S3라면 공개 읽기 권한 확인 |
| 로그에 S3 오류 (`AccessDenied`, `NoSuchBucket`) | 자격 증명·버킷 이름·리전, `S3_ENDPOINT` 확인 |

## 그 밖에

- **화면이 예전 버전으로 보임**: 클라이언트는 서비스 워커로 캐시됩니다. 새 버전은 다음 접속 때 적용되며,
  바로 확인하려면 강력 새로 고침(Ctrl+Shift+R)을 하세요. 웹 서버가 `index.html`, `service-worker.js`,
  `config.js`를 `Cache-Control: no-cache`로 보내는지 확인하세요.
- **게시판을 새로 만들었는데 메뉴에 없음**: 게시판 목록은 최대 `BOARDS_CACHE_SECONDS`(기본 5분) 캐시됩니다.
  관리 페이지에서 "메뉴에 표시"가 켜져 있는지도 확인하세요.
