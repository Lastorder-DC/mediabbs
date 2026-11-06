# 설정 레퍼런스

백엔드는 시작할 때 환경 변수(그리고 작업 디렉터리의 `.env` 파일)를 한 번 읽고 검증합니다.
값이 잘못되면 `Configuration error: <이유>`를 출력하고 종료하므로, 설정을 바꾼 뒤에는 로그를 확인하세요.

- 빈 값(`KEY=`)은 설정하지 않은 것으로 취급합니다.
- 참/거짓 값은 `true`/`false`, `1`/`0`, `yes`/`no`, `on`/`off`를 받습니다.
- 목록은 쉼표(`,`)로 구분합니다. 이름공간 목록은 `|`도 받습니다.
- **`"`나 `$`가 들어간 값**(JSON, `/wiki/$1`, 일부 비밀번호)은 작은따옴표로 감싸세요: `KEY='값'`.
  `.env` 파일은 셸처럼 해석돼 따옴표가 사라지고 `$1` 같은 부분이 치환되기 때문입니다.
  작은따옴표 한 겹은 백엔드가 벗기므로 `docker run --env-file`에서도 같은 파일이 동작합니다.
- 클라이언트(화면)에 필요한 값은 백엔드가 `GET /config`로 내려주므로, **화면에 관한 설정도 모두 여기서** 합니다.

**목차**:
[서버](#서버) ·
[MediaWiki](#mediawiki) ·
[OAuth](#oauth) ·
[관리자](#관리자) ·
[쿠키](#쿠키) ·
[화면](#화면클라이언트) ·
[업로드](#업로드) ·
[예시 모음](#예시-모음)

---

## 서버

| 변수 | 기본값 | 설명 |
|---|---|---|
| `BIND_ADDRESS` | **필수** | 수신 주소. 예: `127.0.0.1:8080`. 예전 이름 `HOST`도 인식 |
| `DATABASE_URL` | **필수** | `mysql://사용자:비밀번호@호스트[:포트]/DB이름`. 비밀번호의 특수문자는 URL 인코딩(`@`→`%40`) |
| `MIGRATE_ON_START` | `false` | 시작할 때 DB 마이그레이션 자동 적용. 끄면 `mediabbs migrate`로 직접 실행 |
| `CORS_ALLOWED_ORIGINS` | (모두 허용) | 클라이언트 origin 목록. 예: `https://bbs.example.org`. 예전 이름 `CORS`(`|` 구분)도 인식. **실서비스에서는 반드시 설정** — 비워 두면 어느 사이트든 로그인 쿠키를 실어 API를 호출할 수 있습니다 |
| `TRUST_PROXY` | loopback | `X-Forwarded-For`/`Forwarded` 헤더를 믿을 프록시. 아래 설명 참고 |
| `SENTRY_DSN` | (끔) | Sentry 오류 보고 DSN. 예전 이름 `SENTRY_URL`도 인식 |
| `RUST_LOG` | `actix_web=info,mediabbs=info` | 로그 수준 ([env_logger](https://docs.rs/env_logger) 문법). 디버깅 시 `mediabbs=debug` |

### TRUST_PROXY

비로그인 글쓴이의 IP 표시와 위키 IP 차단 확인에 쓰는 "실제 사용자 IP"를 정합니다.
백엔드는 연결한 상대(프록시)가 신뢰 목록에 있을 때만 전달 헤더를 따라가며, 신뢰할 수 없는 첫 주소를
사용자 IP로 봅니다. 사용자가 헤더를 위조해도 무시됩니다.

| 값 | 의미 |
|---|---|
| (비워 둠) | `127.0.0.0/8`, `::1` 만 신뢰 — 같은 서버의 nginx 뒤에 둘 때 |
| `false` | 헤더를 쓰지 않음 — 프록시 없이 직접 노출할 때 |
| `true` | 모든 경유지를 신뢰 — 백엔드에 프록시를 통해서만 접근할 수 있을 때만 |
| CIDR 목록 | 예: `172.17.0.1,10.0.0.0/8,::1` |

---

## MediaWiki

| 변수 | 기본값 | 설명 |
|---|---|---|
| `MW_BASE_URL` | **필수** | 위키의 공개 주소 (스킴+호스트). 예: `https://wiki.example.org` |
| `MW_SCRIPT_PATH` | (빈 값) | 위키의 `$wgScriptPath`. `…/w/api.php`면 `/w`, `…/api.php`면 빈 값 |
| `MW_API_URL` | 공개 주소로 계산 | 백엔드가 `api.php`에 접속할 주소. 같은 서버/내부망이라 다른 주소로 접속해야 할 때만. 예: `http://10.0.0.5/w/api.php` |
| `MW_REST_URL` | 공개 주소로 계산 | 백엔드가 `rest.php`에 접속할 주소. 위와 같음 |
| `MW_ARTICLE_PATH` | 위키에서 자동 탐지 | 문서 주소 형식, `$1`이 문서 이름. 예: `'/wiki/$1'` (작은따옴표 필수). 자동 탐지가 실패하면 `index.php?title=$1` 사용 |
| `CHECK_IP_BLOCKS` | `true` | 비로그인 글쓰기 때 위키의 IP 차단 목록 확인 (`list=blocks`, 대역 차단 포함) |
| `MW_USER_AGENT` | `mediabbs/<버전> (+저장소 URL)` | 위키에 보내는 User-Agent |

- 브라우저에 보내는 위키 주소(로그인 화면, 문서 링크)는 항상 `MW_BASE_URL` + `MW_SCRIPT_PATH`로 만듭니다.
  `MW_API_URL`/`MW_REST_URL`은 백엔드 자신의 접속에만 쓰입니다.
- 위키 정보(사이트 이름, 로고, 언어, 문서 주소 형식, 이름공간)는 `api.php?meta=siteinfo`로 가져와
  1시간 동안 캐시합니다. 위키에 접속하지 못하면 1분 뒤 다시 시도하고, 그동안은 이전 값을 씁니다.
- 위키 문서 링크는 `User:`, `Special:Contributions` 같은 **영어 정식 이름**으로 만듭니다.
  MediaWiki는 언어와 상관없이 정식 이름을 받으므로(한국어 위키라면 `사용자:`로 이동) 따로 설정할 것이 없습니다.

---

## OAuth

| 변수 | 기본값 | 설명 |
|---|---|---|
| `OAUTH_CLIENT_ID` | **필수** | 위키에 등록한 OAuth 2.0 컨슈머의 클라이언트 키 |
| `OAUTH_CLIENT_SECRET` | **필수** | 클라이언트 비밀 키. **외부에 노출하지 마세요** |
| `OAUTH_PUBLIC_KEY_PATH` | `pubkey.pem` | 위키의 `$wgOAuth2PublicKey`와 **같은** 공개키(PEM) 파일 경로. 상대 경로는 작업 디렉터리 기준 |

로그인 쿠키에 담긴 액세스 토큰(JWT)은 이 공개키와 클라이언트 키(토큰의 `aud`)로 백엔드에서 직접 검증합니다.
위키가 키를 바꾸면 이 파일도 바꾸고 백엔드를 재시작해야 합니다.

---

## 관리자

| 변수 | 기본값 | 설명 |
|---|---|---|
| `ADMIN_GROUPS` | `sysop` | 게시판 관리자로 인정할 위키 사용자 그룹 목록. 예: `sysop,boardmanager` |
| `ADMIN_RIGHTS` | (없음) | 관리자로 인정할 위키 권한 목록 |

둘 중 하나라도 해당하면 관리자입니다. 관리자는 주제 숨기기·잠그기·종료·상단 고정, 의견 숨기기와
숨긴 의견 보기를 할 수 있습니다.

> `ADMIN_RIGHTS`는 신원 확인용 컨슈머(이 게시판의 권장 설정)에서는 **동작하지 않습니다.**
> 위키의 사용자 정보 응답에 권한 목록이 비어 있기 때문입니다. `ADMIN_GROUPS`를 쓰세요.

권한은 요청마다 위키에 확인하므로, 위키에서 그룹을 바꾸면 바로 적용됩니다
(화면 표시는 최대 1분 늦을 수 있습니다).

---

## 쿠키

로그인하면 백엔드가 `access_token`, `refresh_token` 두 쿠키(HttpOnly)를 저장합니다.

| 변수 | 기본값 | 설명 |
|---|---|---|
| `COOKIE_SECURE` | `true` | HTTPS에서만 쿠키 전송. HTTP로 운영하는 경우(테스트 등)에만 `false`. `localhost`는 HTTP라도 `true`로 동작 |
| `COOKIE_SAMESITE` | `Lax` | `Lax`, `Strict`, `None`. 클라이언트와 백엔드가 서로 **다른 사이트**일 때만 `None` (`COOKIE_SECURE=true` 필요) |
| `COOKIE_DOMAIN` | (현재 호스트) | 쿠키 도메인. 보통 비워 둡니다 |
| `COOKIE_MAX_AGE_DAYS` | `28` | 쿠키 유지 기간(일). 실제 로그인 유지 기간은 위키의 `$wgOAuth2RefreshTokenTTL`(기본 1개월)과 짧은 쪽 |

"같은 사이트"는 등록 도메인이 같다는 뜻입니다. `bbs.example.org`와 `api.example.org`는 같은 사이트,
`bbs.foo.com`과 `api.bar.net`은 다른 사이트입니다.

---

## 화면(클라이언트)

`GET /config`로 클라이언트에 전달됩니다. 바꾼 뒤 백엔드를 재시작하면 화면은 다시 빌드하지 않아도 반영됩니다
(브라우저 캐시 때문에 최대 5분 늦을 수 있습니다).

| 변수 | 기본값 | 설명 |
|---|---|---|
| `SITE_NAME` | `<위키 이름> 게시판` | 화면 제목·브라우저 탭에 쓰는 게시판 사이트 이름 |
| `SITE_LOCALE` | `ko-KR` | 날짜 표시 형식과 편집기 언어(`ko…`이면 한국어, 그 밖은 영어). 화면 문구는 한국어로 고정 |
| `DEFAULT_BOARD` | 첫 활성 게시판 | `/`로 접속했을 때 보여 줄 게시판 이름 |
| `NAV_LINKS` | (없음) | 상단 메뉴에 추가할 링크 (JSON). 아래 참고 |
| `RECENT_CHANGES_ENABLED` | `true` | 오른쪽 "최근 바뀜 / 최근 토론" 패널 표시 |
| `RECENT_CHANGES_NAMESPACES` | 자동 | "최근 바뀜" 탭의 이름공간 번호. 예: `0|4|10|12|14` |
| `RECENT_DISCUSSIONS_NAMESPACES` | 자동 | "최근 토론" 탭의 이름공간 번호. 예: `1|3|5` |
| `SITENOTICE_ENABLED` | `true` | 위키의 사이트 공지를 게시판 위에 표시 |
| `SITENOTICE_PAGE` | `MediaWiki:Sitenotice` | 공지로 쓸 위키 문서 |
| `BOARDS_CACHE_SECONDS` | `300` | 게시판 목록(`GET /boards`)의 브라우저 캐시 시간(초) |

### 이름공간 자동 설정

설정하지 않으면 위키의 이름공간 정보로 정합니다.

- 최근 바뀜: 본문 이름공간(`$wgContentNamespaces`) + 프로젝트(4), 틀(10), 도움말(12), 분류(14)
- 최근 토론: 모든 토론 이름공간(홀수 번호)

### NAV_LINKS

상단 메뉴의 "도구" 오른쪽에 붙는 항목입니다. JSON 배열이며 각 항목은 링크이거나 드롭다운입니다.

```json
[
  { "label": "지도", "url": "https://maps.example.org", "icon": "map" },
  {
    "label": "도움말", "icon": "question",
    "children": [
      { "label": "문법 도움말", "url": "https://wiki.example.org/wiki/도움말:문법" },
      { "label": "이용 규칙", "url": "https://wiki.example.org/wiki/프로젝트:규칙" }
    ]
  }
]
```

- `icon`(선택): [Font Awesome 5](https://fontawesome.com/v5/search?m=free) 무료 아이콘 이름 (`fa-` 없이)
- `.env` 파일에서는 **작은따옴표로 감싸 한 줄로** 씁니다: `NAV_LINKS='[{"label":"지도","url":"https://maps.example.org","icon":"map"}]'`

위키 링크(최근 바뀜, 임의 문서, 파일 업로드, 특수 문서 목록, 환경설정 등)와 게시판 목록 메뉴는 자동으로 만들어집니다.

---

## 업로드

글 편집기에 붙여 넣거나 끌어다 놓은 이미지를 저장합니다.

| 변수 | 기본값 | 설명 |
|---|---|---|
| `UPLOAD_ENABLED` | `true` | `false`면 업로드를 끄고 저장소 설정도 필요 없음 |
| `UPLOAD_REQUIRE_LOGIN` | `true` | 로그인한 사용자만 업로드 |
| `UPLOAD_MAX_BYTES` | `10485760` (10MiB) | 파일 하나의 최대 크기(바이트). 프록시의 요청 크기 제한은 이 값의 약 4/3 이상으로 |
| `UPLOAD_ALLOWED_EXTENSIONS` | `png,jpg,jpeg,gif,webp` | 허용 확장자. png/jpg/gif/webp는 파일 내용도 검사함. **svg·html은 넣지 마세요**(스크립트 삽입 위험) |
| `STORAGE_BACKEND` | `s3` | `s3` 또는 `local` |
| `UPLOAD_PUBLIC_URL` | AWS S3면 자동 | 저장된 파일을 브라우저가 받을 주소의 앞부분. `local`과 `S3_ENDPOINT` 사용 시 필수 |

차단된 사용자와 (비로그인 업로드를 허용한 경우) 차단된 IP는 업로드할 수 없습니다.
파일 이름은 무작위로 새로 정해집니다.

### S3 (`STORAGE_BACKEND=s3`)

| 변수 | 기본값 | 설명 |
|---|---|---|
| `S3_BUCKET` | **필수** | 버킷 이름 |
| `S3_REGION` | AWS면 필수 | AWS 리전(예: `ap-northeast-2`). `S3_ENDPOINT`를 쓰면 기본 `us-east-1` |
| `S3_ENDPOINT` | (AWS) | S3 호환 스토리지 주소. 경로 방식(`/버킷/키`)으로 접속 |
| `S3_KEY_PREFIX` | `bbs/` | 객체 키 앞부분. `/`이면 접두사 없음 |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | — | 자격 증명. `~/.aws/credentials`나 EC2 인스턴스 역할도 사용 가능 |

필요한 권한은 해당 접두사에 대한 `s3:PutObject` 하나입니다. 파일은 `UPLOAD_PUBLIC_URL`로 공개적으로
읽을 수 있어야 하므로, 버킷(또는 접두사)을 공개 읽기로 하거나 CDN을 앞에 두세요.

**AWS S3**

```ini
STORAGE_BACKEND=s3
S3_REGION=ap-northeast-2
S3_BUCKET=my-bbs-images
# UPLOAD_PUBLIC_URL 기본값: https://my-bbs-images.s3.ap-northeast-2.amazonaws.com/
# CloudFront를 쓴다면: UPLOAD_PUBLIC_URL=https://dxxxx.cloudfront.net/
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
```

**Cloudflare R2**

```ini
STORAGE_BACKEND=s3
S3_ENDPOINT=https://<계정 ID>.r2.cloudflarestorage.com
S3_REGION=auto
S3_BUCKET=my-bbs-images
UPLOAD_PUBLIC_URL=https://images.example.org/     # 버킷에 연결한 공개 도메인
AWS_ACCESS_KEY_ID=<R2 API 토큰의 Access Key ID>
AWS_SECRET_ACCESS_KEY=<R2 API 토큰의 Secret Access Key>
```

**MinIO**

```ini
STORAGE_BACKEND=s3
S3_ENDPOINT=https://minio.example.org
S3_BUCKET=bbs
UPLOAD_PUBLIC_URL=https://minio.example.org/bbs/
```

### 로컬 디스크 (`STORAGE_BACKEND=local`)

| 변수 | 기본값 | 설명 |
|---|---|---|
| `LOCAL_UPLOAD_DIR` | `./uploads` | 저장 디렉터리. 백엔드 실행 사용자가 쓸 수 있어야 함 |

파일은 백엔드가 `GET /uploads/<파일명>`으로 직접 내보냅니다(`nosniff`, sandbox CSP 헤더 포함).
`UPLOAD_PUBLIC_URL`은 브라우저가 그 경로에 접근하는 주소입니다.

```ini
STORAGE_BACKEND=local
LOCAL_UPLOAD_DIR=/opt/mediabbs/uploads
UPLOAD_PUBLIC_URL=https://bbs.example.org/api/uploads/
```

트래픽이 많다면 nginx가 디렉터리를 직접 서빙하도록 하고 `UPLOAD_PUBLIC_URL`을 그 주소로 바꿔도 됩니다.
이때도 `add_header X-Content-Type-Options nosniff;`를 붙이세요.

---

## 예시 모음

### 작은 위키 (단일 서버, 업로드는 로컬)

```ini
BIND_ADDRESS=127.0.0.1:8080
DATABASE_URL=mysql://mediabbs:CHANGE_ME@localhost/mediabbs
MIGRATE_ON_START=true
CORS_ALLOWED_ORIGINS=https://bbs.example.org

MW_BASE_URL=https://wiki.example.org
MW_SCRIPT_PATH=/w
OAUTH_CLIENT_ID=0123456789abcdef0123456789abcdef
OAUTH_CLIENT_SECRET=0123456789abcdef0123456789abcdef01234567
OAUTH_PUBLIC_KEY_PATH=oauth2.pub

ADMIN_GROUPS=sysop
DEFAULT_BOARD=free

STORAGE_BACKEND=local
LOCAL_UPLOAD_DIR=/opt/mediabbs/uploads
UPLOAD_PUBLIC_URL=https://bbs.example.org/api/uploads/
```

### 위키와 같은 내부망, 관리자 그룹 분리, R2 업로드

```ini
BIND_ADDRESS=127.0.0.1:8080
DATABASE_URL=mysql://mediabbs:CHANGE_ME@db.internal/mediabbs
CORS_ALLOWED_ORIGINS=https://bbs.example.org
TRUST_PROXY=10.0.0.0/8

MW_BASE_URL=https://wiki.example.org
MW_SCRIPT_PATH=
MW_API_URL=http://wiki.internal/api.php
MW_REST_URL=http://wiki.internal/rest.php
OAUTH_CLIENT_ID=...
OAUTH_CLIENT_SECRET=...
OAUTH_PUBLIC_KEY_PATH=/etc/mediabbs/oauth2.pub

ADMIN_GROUPS=sysop,boardmanager
SITE_NAME=예시 위키 토론장
NAV_LINKS='[{"label":"도움말","icon":"question","children":[{"label":"게시판 규칙","url":"https://wiki.example.org/wiki/프로젝트:게시판"}]}]'

STORAGE_BACKEND=s3
S3_ENDPOINT=https://<계정 ID>.r2.cloudflarestorage.com
S3_REGION=auto
S3_BUCKET=bbs-images
UPLOAD_PUBLIC_URL=https://images.example.org/
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
```

### 리브레 위키 설정 재현

[`contrib/librewiki/env.librewiki`](../contrib/librewiki/env.librewiki)를 보세요.
