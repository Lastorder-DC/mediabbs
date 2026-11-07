# 운영 안내

## 명령줄 도구

`mediabbs` 바이너리 하나로 서버 실행과 관리 작업을 모두 합니다. 서버 이외의 명령은
`DATABASE_URL`만 있으면 됩니다(환경 변수 또는 작업 디렉터리의 `.env`).

```text
mediabbs [serve]                          서버 실행 (기본)
mediabbs migrate                          DB 마이그레이션 적용
mediabbs board list                       게시판 목록 (메뉴 순서)
mediabbs board add <이름> <표시 이름>     게시판 만들기
mediabbs board rename <이름> <표시 이름>  표시 이름 바꾸기
mediabbs board set-name <이름> <새 이름>  이름(주소) 바꾸기
mediabbs board disable <이름>             게시판을 읽기 전용(보관)으로
mediabbs board enable <이름>              다시 쓰기 가능하게
mediabbs board hide <이름>                메뉴에서 빼기 (주소로는 계속 접근 가능)
mediabbs board show <이름>                메뉴에 다시 표시
mediabbs board icon <이름> <아이콘|->     Bootstrap Icons 아이콘 (-는 없애기)
mediabbs board separator <이름> on|off    메뉴에서 이 게시판 위에 구분선
mediabbs board move <이름> <위치>         메뉴 순서 바꾸기 (1 = 맨 위)
mediabbs board permission <이름> write|comment all|users|admins
                                          새 주제 / 의견을 쓸 수 있는 사람
mediabbs board delete <이름>              주제가 없는 게시판 삭제
mediabbs help | version
```

Docker에서는 이미지 이름 뒤에 붙입니다.

```sh
docker run --rm --env-file mediabbs.env mediabbs board list
docker exec mediabbs mediabbs board list        # 실행 중인 컨테이너에서
```

## 게시판 관리

게시판은 **웹 관리 페이지**나 명령줄로 관리합니다. 둘은 같은 설정을 바꿉니다.

### 관리 페이지

게시판 관리자(`ADMIN_GROUPS` / `ADMIN_RIGHTS`)로 로그인하면 사용자 메뉴(Liberty)나 상단의
사용자 이름 옆(Simple)에 **게시판 관리** 링크가 생깁니다(`https://bbs.example.org/admin`).

**게시판** 탭

- 목록: 메뉴 순서대로 보입니다. 끌어 놓거나 ↑↓ 버튼으로 순서를 바꾼 뒤 **순서 저장**을 누릅니다.
- ⚙ 버튼이나 **새 게시판**으로 아래 설정을 바꿉니다.

| 설정 | 설명 |
|---|---|
| 표시 이름 | 화면에 보이는 이름 |
| 이름 (주소) | `https://bbs.example.org/<이름>`. 바꾸면 기존 게시판·글 주소가 열리지 않습니다 |
| 아이콘 | 메뉴와 게시판 제목에서 이름 왼쪽에 보일 [Bootstrap Icons](https://icons.getbootstrap.com/) 이름(예: `megaphone`, `chat-dots`). 입력하면 미리 보기가 나옵니다 |
| 글 작성 / 의견 작성 | 새 주제 / 의견을 쓸 수 있는 사람: 누구나(비로그인 포함), 로그인 사용자, 관리자만 |
| 메뉴에 표시 | 끄면 메뉴의 게시판 목록에서 빠집니다. 주소로는 계속 들어갈 수 있습니다 |
| 위에 구분선 | 메뉴의 게시판 목록에서 이 게시판 위에 구분선을 넣습니다(게시판 묶음 나누기) |
| 보관 (읽기 전용) | 새 주제와 의견을 쓸 수 없게 합니다 |

- **삭제**는 주제가 하나도 없는 게시판만 됩니다. 쓰던 게시판은 보관하거나 메뉴에서 빼세요.

예: 공지 게시판은 "글 작성: 관리자만, 의견 작성: 로그인 사용자", 맨 위로 옮기고 아이콘 `megaphone`.
그 아래 일반 게시판 묶음은 첫 게시판에 "위에 구분선"을 켭니다.

**메뉴** 탭

상단 메뉴의 항목과 순서를 스킨별로 정합니다.

- 스킨을 고르면 지금 쓰이는 메뉴와 그 출처(저장된 메뉴 / `config.js` / 스킨 기본값)가 보입니다.
- 항목을 ↑↓로 옮기고, ✕로 빼고, 기본 항목(최근 바뀜, 임의 문서, 게시판 목록, 도구, 위키 대문,
  `NAV_LINKS`)이나 링크·드롭다운을 추가합니다. 기본 항목의 이름은 비워 두면 번역된 기본 이름을 씁니다.
- **기본 (모든 스킨)** 메뉴를 저장하면 자기 메뉴가 없는 모든 스킨에 쓰입니다.
- 언어별 이름 같은 고급 설정은 **JSON으로 편집**에서 합니다(형식은 클라이언트 `config.js`의 `menus`와 같음).
- **저장된 메뉴 지우기**를 하고 저장하면 `config.js` 또는 스킨 기본 메뉴로 돌아갑니다.

우선순위: 관리 페이지에 저장한 스킨 메뉴 → 저장한 기본 메뉴 → `config.js`의 `menus` → 스킨 기본 메뉴.

**관리 기록** 탭

관리 동작(주제 숨기기·고정 등, 의견 숨기기, 게시판·메뉴 관리)을 최신순으로 보여 줍니다. 누가, 언제,
무엇을 했는지와 대상 주제·게시판 링크가 나오고, 종류(주제 관리 / 의견 관리 / 게시판 설정 / 메뉴)로 거를 수
있습니다. **더 보기**로 이전 기록을 불러옵니다. IP 주소는 보여 주지 않습니다(DB에만 있음).

설정과 게시판 목록은 브라우저에 캐시되므로 다른 사용자에게는 최대 5분(`BOARDS_CACHE_SECONDS`) 뒤에
반영됩니다. 바꾼 관리자 본인의 브라우저에는 바로 반영됩니다.

### 명령줄

```sh
mediabbs board add free 자유게시판
mediabbs board add notice 공지사항
mediabbs board permission notice write admins
mediabbs board icon notice megaphone
mediabbs board move notice 1
mediabbs board separator free on
mediabbs board list
#    2  notice               공지사항  (icon=megaphone, write=admins)
#    1  free                 자유게시판  (separator)
```

- **이름**은 주소에 쓰입니다(`https://bbs.example.org/free`). 소문자 영문으로 시작하고 소문자·숫자·`-`·`_`만
  쓸 수 있으며, 클라이언트 경로와 겹치는 `auth`, `admin`, `api`, `uploads`, `config`, `search`는 쓸 수 없습니다.
  `board set-name`으로 바꿀 수 있지만 기존 주소가 열리지 않게 됩니다.
- **표시 이름**은 화면에 보이는 이름이며 언제든 `board rename`으로 바꿀 수 있습니다.
- 상단 메뉴의 게시판 목록은 게시판 설정(순서, 메뉴 표시, 아이콘, 구분선)대로 자동으로 만들어집니다.
- 더 쓰지 않는 게시판은 `board disable`로 **보관** 상태로 두세요. 보관된 게시판은 읽을 수 있지만
  새 주제와 의견을 쓸 수 없습니다. `board delete`는 주제가 없는 게시판만 지웁니다.
- `/`로 접속했을 때 보여 줄 게시판은 `DEFAULT_BOARD`로 정합니다(기본: 메뉴에 보이는 첫 번째 활성 게시판).

## 관리자 기능

`ADMIN_GROUPS`에 속한 위키 사용자는 게시판 관리자입니다. 주제 화면의 **관리자 메뉴**와
의견 오른쪽의 메뉴에서 다음을 할 수 있습니다.

| 동작 | 대상 | 효과 |
|---|---|---|
| 숨기기 / 숨김 해제 | 주제 | 목록과 본문에서 감춤. 주소로도 볼 수 없음 |
| 잠그기 / 잠금 해제 | 주제 | 의견 추가 금지 (토론 일시 중단) |
| 종료 / 종료 취소 | 주제 | 의견 추가 금지 (토론 마무리) |
| 상단 고정 / 해제 | 주제 | 게시판 목록 맨 위에 표시 |
| 숨기기 / 숨기기 취소 | 의견 | 내용을 "관리자에 의해 숨겨졌습니다"로 대체 |
| 내용 보기 | 숨긴 의견 | 관리자만 원래 내용 확인 |
| 사용자 차단 | 의견 작성자 | 위키의 `Special:Block`으로 이동 (차단은 위키에서) |

관리 동작(게시판·메뉴 관리 포함)은 DB의 `logs` 테이블에 기록됩니다(누가, 언제, 어떤 동작, 대상 ID, IP).
관리 페이지의 **관리 기록** 탭에서 볼 수 있고(IP 제외), IP까지 보려면 직접 조회하세요.

```sql
SELECT l.created_at, t.name AS action, l.user_name, l.content
FROM logs l JOIN log_types t ON t.id = l.log_type_id
ORDER BY l.id DESC LIMIT 50;
```

### 차단

차단은 위키에서 합니다. 백엔드는 글을 쓸 때마다 위키에 확인합니다.

- 로그인 사용자: 위키 계정이 차단돼 있으면 글·의견·업로드 불가
- 비로그인 사용자: 위키에서 그 IP(또는 IP 대역)가 차단돼 있으면 불가 (`CHECK_IP_BLOCKS=true`, 기본값)

차단을 풀면 즉시 다시 쓸 수 있습니다.

## 백업

| 대상 | 방법 |
|---|---|
| DB | `mysqldump --single-transaction mediabbs > mediabbs.sql` (트리거 포함) |
| 로컬 업로드 | `LOCAL_UPLOAD_DIR` 디렉터리 (S3라면 버킷 쪽 정책으로) |
| 설정 | `.env`, 위키 OAuth 공개키 |

사용자 계정 정보는 위키에 있으므로 게시판 쪽에는 백업할 것이 없습니다.

## 업그레이드

```sh
cd mediabbs && git pull
# 직접 빌드
cargo build --release && sudo install target/release/mediabbs /opt/mediabbs/mediabbs
cd /opt/mediabbs && sudo -u mediabbs ./mediabbs migrate && sudo systemctl restart mediabbs
# Docker
docker build -t mediabbs . && docker run --rm --env-file mediabbs.env mediabbs migrate
docker rm -f mediabbs && docker run -d --name mediabbs ... mediabbs
```

`MIGRATE_ON_START=true`라면 `migrate` 단계는 생략해도 됩니다. 클라이언트도 같은 시기에 함께 업데이트하세요
(백엔드와 클라이언트는 API 형식을 공유합니다).

## LibreBBS에서 옮겨 오기

기존 LibreBBS(librebbs) DB는 **그대로** 쓸 수 있습니다. 예전에 게시판을 넣던 마이그레이션은 이제 아무 일도
하지 않고, 이미 적용된 마이그레이션은 다시 실행되지 않으므로 데이터가 바뀌지 않습니다.

1. 백엔드를 MediaBBS로 바꾸고 `mediabbs migrate`를 실행합니다(새로 적용할 것이 없으면 그대로 끝남).
2. 환경 변수를 아래 표에 맞게 고칩니다. [`contrib/librewiki/env.librewiki`](../contrib/librewiki/env.librewiki)에
   리브레 위키 설정을 재현한 예시가 있습니다.
3. 클라이언트를 mediabbs-client로 바꿉니다. 클라이언트에는 이제 백엔드 주소만 설정합니다
   (`VUE_APP_CLIENT_ID`는 없어졌습니다).

| LibreBBS | MediaBBS | 비고 |
|---|---|---|
| `HOST` | `BIND_ADDRESS` | 예전 이름도 인식 |
| `CORS` (`|` 구분) | `CORS_ALLOWED_ORIGINS` | 예전 이름도 인식 |
| `SENTRY_URL` (필수) | `SENTRY_DSN` (선택) | 예전 이름도 인식 |
| (librewiki.net 고정) | `MW_BASE_URL`, `MW_SCRIPT_PATH` | **새로 필수** |
| `pubkey.pem` (고정) | `OAUTH_PUBLIC_KEY_PATH` | 기본값은 그대로 `pubkey.pem` |
| (`boardmanager` 고정) | `ADMIN_GROUPS` | 기본값이 `sysop`으로 바뀜. 리브레 위키는 `boardmanager` |
| (image.librewiki.net 고정) | `UPLOAD_PUBLIC_URL` | 리브레 위키는 `https://image.librewiki.net/` |
| (`bbs/` 고정) | `S3_KEY_PREFIX` | 기본값은 그대로 `bbs/` |
| (비로그인 업로드 허용) | `UPLOAD_REQUIRE_LOGIN` | 기본값이 `true`로 바뀜 |
| (쿠키 Secure 없음) | `COOKIE_SECURE` | 기본값 `true`. HTTPS가 아니면 `false` |
| (모든 프록시 헤더 신뢰) | `TRUST_PROXY` | 기본값은 loopback만 신뢰 |

API 응답의 필드 이름이 camelCase(`displayName`, `isHidden` 등)로 바뀌었습니다. 예전 librebbs-client는
새 백엔드와 함께 쓸 수 없습니다.
