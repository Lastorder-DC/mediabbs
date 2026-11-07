# 설치 안내

MediaBBS를 실제 서비스에 설치하는 과정입니다. 순서대로 진행하세요.

1. [준비물과 구성 결정](#1-준비물과-구성-결정)
2. [MediaWiki 준비 (OAuth 2.0)](#2-mediawiki-준비-oauth-20)
3. [데이터베이스 준비](#3-데이터베이스-준비)
4. [백엔드 설치](#4-백엔드-설치)
5. [클라이언트 배포](#5-클라이언트-배포)
6. [리버스 프록시(nginx)](#6-리버스-프록시nginx)
7. [설치 확인](#7-설치-확인)

> 로컬에서 먼저 동작을 확인하고 싶다면 [dev/README.md](../dev/README.md)의
> docker-compose 환경을 쓰세요. 아래 과정 전체가 자동으로 구성돼 있습니다.

---

## 1. 준비물과 구성 결정

### 필요한 것

| 항목 | 요구 사항 |
|---|---|
| MediaWiki | Extension:OAuth의 OAuth 2.0을 쓸 수 있는 버전 (MediaWiki 1.35 이상, **1.43 LTS에서 검증**). 위키 서버 관리자 권한 |
| 데이터베이스 | MySQL 5.7+ 또는 MariaDB 10.3+ (utf8mb4) |
| 백엔드 실행 환경 | Linux. Docker, 또는 직접 빌드 시 Rust 1.94 이상과 `libmysqlclient`/`libmariadb`, OpenSSL |
| 클라이언트 | 정적 파일을 서빙할 웹 서버(nginx 등), Docker, 또는 GitHub Pages |
| HTTPS | 실서비스에서는 필수 (로그인 쿠키에 `Secure`가 기본 적용됨) |
| 업로드 저장소 (선택) | AWS S3, S3 호환 스토리지, 또는 백엔드 서버의 디스크 |

### 주소 구성 정하기

브라우저는 클라이언트(화면)와 백엔드(API) 두 곳에 접속합니다. 로그인 쿠키는 백엔드
주소에 저장되므로, 두 주소의 관계에 따라 설정이 달라집니다.

| 구성 | 예 | 권장 | 비고 |
|---|---|---|---|
| **A. 같은 주소, API는 경로로** | 화면 `https://bbs.example.org`, API `https://bbs.example.org/api` | ★ 가장 간단 | CORS·쿠키 설정 거의 불필요. 이 문서의 기본 예시 |
| B. 같은 도메인의 서로 다른 서브도메인 | 화면 `https://bbs.example.org`, API `https://bbs-api.example.org` | 가능 | `CORS_ALLOWED_ORIGINS` 필요. 쿠키는 기본값(`SameSite=Lax`)으로 동작 |
| C. 서로 다른 도메인 | 화면 `https://bbs.foo.com`, API `https://api.bar.net` | 비권장 | `COOKIE_SAMESITE=None` 필요. 브라우저의 서드파티 쿠키 차단에 막힐 수 있음 |

위키 주소는 어느 쪽과 같아도, 달라도 상관없습니다.

이 문서에서는 다음 주소를 예로 듭니다.

- 위키: `https://wiki.example.org` (스크립트 경로 `/w`, 문서 주소 `/wiki/문서명`)
- 게시판: `https://bbs.example.org` (화면), `https://bbs.example.org/api` (API, 구성 A)

---

## 2. MediaWiki 준비 (OAuth 2.0)

### 2.1 Extension:OAuth 설치

위키 버전에 맞는 브랜치를 받습니다. 예시는 MediaWiki 1.43입니다.

```sh
cd /path/to/mediawiki/extensions
git clone -b REL1_43 https://gerrit.wikimedia.org/r/mediawiki/extensions/OAuth
cd OAuth
composer install --no-dev      # OAuth 2.0 라이브러리 설치. 꼭 필요합니다.
```

[Special:ExtensionDistributor/OAuth](https://www.mediawiki.org/wiki/Special:ExtensionDistributor/OAuth)
에서 받은 압축 파일을 써도 되지만, 이 경우에도 `composer install --no-dev`가 필요합니다.

### 2.2 OAuth 2.0 서명 키 만들기

위키가 액세스 토큰(JWT)에 서명할 RSA 키 쌍입니다. **공개키는 나중에 MediaBBS 백엔드에도 그대로 넣습니다.**

```sh
openssl genrsa -out /etc/mediawiki/oauth2.key 2048
openssl rsa -in /etc/mediawiki/oauth2.key -pubout -out /etc/mediawiki/oauth2.pub
chown www-data: /etc/mediawiki/oauth2.key   # 웹 서버 사용자만 읽을 수 있게
chmod 600 /etc/mediawiki/oauth2.key
```

### 2.3 LocalSettings.php 설정

```php
wfLoadExtension( 'OAuth' );

// OAuth 2.0 서명 키 (키 내용 또는 파일 경로)
$wgOAuth2PrivateKey = '/etc/mediawiki/oauth2.key';
$wgOAuth2PublicKey  = '/etc/mediawiki/oauth2.pub';

// 컨슈머(앱) 등록·승인 권한. 기본으로는 아무 그룹에도 없습니다.
$wgGroupPermissions['sysop']['mwoauthproposeconsumer'] = true;
$wgGroupPermissions['sysop']['mwoauthupdateownconsumer'] = true;
$wgGroupPermissions['sysop']['mwoauthmanageconsumer'] = true;

// (선택) 액세스 토큰 수명. 기본 1시간. 만료되면 MediaBBS가 자동으로 갱신합니다.
// $wgOAuth2GrantExpirationInterval = 'PT1H';
// (선택) 리프레시 토큰 수명. 기본 1개월. 이 기간 동안 접속이 없으면 다시 로그인해야 합니다.
// $wgOAuth2RefreshTokenTTL = 'P1M';
```

설정 후 DB 테이블을 만듭니다.

```sh
php maintenance/run.php update      # 1.39 이하: php maintenance/update.php
```

### 2.4 (선택) 게시판 관리자 그룹 만들기

위키 관리자(`sysop`)가 그대로 게시판 관리자가 되는 것이 기본입니다. 게시판 관리만 맡길
별도 그룹이 필요하면 만들고, 나중에 백엔드의 `ADMIN_GROUPS`에 넣습니다.

```php
$wgGroupPermissions['boardmanager']['read'] = true;   // 그룹을 생성하는 효과
$wgAddGroups['sysop'][] = 'boardmanager';             // 관리자가 이 그룹을 부여할 수 있게
$wgRemoveGroups['sysop'][] = 'boardmanager';
```

`MediaWiki:Group-boardmanager` 문서에 그룹 표시 이름을 적어 두면 좋습니다.

### 2.5 OAuth 2.0 컨슈머 등록

관리자 계정으로 로그인한 뒤 **`Special:OAuthConsumerRegistration/propose/oauth2`** 로 이동해
다음처럼 입력합니다. (메뉴 경로: 특수 문서 → OAuth 컨슈머 등록 → OAuth 2.0 클라이언트 제안)

| 항목 | 값 |
|---|---|
| 애플리케이션 이름 | 예: `MediaBBS` (로그인 승인 화면에 표시됨) |
| 애플리케이션 설명 | 자유롭게 |
| OAuth "콜백" URL | **`https://bbs.example.org/auth`** (클라이언트 주소 + `/auth`) |
| 이 소비자는 …에서만 사용 (owner-only) | **체크하지 않음** |
| 클라이언트가 기밀(confidential)임 | **체크** (백엔드가 비밀 키를 보관함) |
| 허용된 승인 유형 | **Authorization code**, **Refresh token** |
| 권한 유형 | 아래 둘 중 하나 |
| 공개 RSA 키 | 비워 둠 |

권한 유형:

- **사용자 신원 확인만, 실명·이메일 접근 포함** (`authonlyprivate`) — 이메일을 받아 로그인
  사용자의 Gravatar 아바타를 보여 줍니다. 승인 화면에 이메일 접근이 표시됩니다.
- **사용자 신원 확인만** (`authonly`) — 이메일을 받지 않습니다. 아바타는 기본 이미지로 표시됩니다.

MediaBBS는 위키 문서를 읽거나 편집하지 않으므로 그 밖의 권한은 필요 없습니다.

제출하면 **클라이언트 애플리케이션 키**(client ID)와 **클라이언트 애플리케이션 비밀 키**(client secret)가
한 번만 표시됩니다. 안전한 곳에 적어 두세요.

마지막으로 **`Special:OAuthManageConsumers/proposed`** 에서 방금 만든 컨슈머를 **승인**합니다.
승인 전에는 로그인이 되지 않습니다.

> 콜백 URL이나 권한을 바꾸려면 `Special:OAuthConsumerRegistration/list`에서 컨슈머를 수정하세요.
> 비밀 키를 잃어버렸다면 같은 화면에서 재설정할 수 있습니다.

### 2.6 위키 쪽 CORS

필요 없습니다. 브라우저가 위키에서 직접 읽는 정보(최근 바뀜, 사이트 공지, 검색 자동완성)는
로그인 없는 읽기 요청이라 `origin=*`로 호출하며, MediaWiki는 별도 설정 없이 이를 허용합니다.

---

## 3. 데이터베이스 준비

```sql
CREATE DATABASE mediabbs CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
CREATE USER 'mediabbs'@'localhost' IDENTIFIED BY 'CHANGE_ME';
GRANT ALL PRIVILEGES ON mediabbs.* TO 'mediabbs'@'localhost';
```

마이그레이션에 트리거 생성(`CREATE TRIGGER`)이 포함돼 있습니다. MySQL에서 바이너리 로그를 쓰고
있다면 `log_bin_trust_function_creators=1`이 필요할 수 있습니다.

테이블은 백엔드의 `mediabbs migrate` 명령이 만듭니다(4.4 참고).

---

## 4. 백엔드 설치

Docker와 직접 빌드 중 하나를 고르세요.

### 4.1 방법 1: Docker

```sh
git clone https://github.com/Lastorder-DC/mediabbs.git
cd mediabbs
docker build -t mediabbs .
```

설정 파일(4.3)을 만든 뒤 실행합니다.

```sh
docker run -d --name mediabbs --restart unless-stopped \
  --env-file /etc/mediabbs/mediabbs.env \
  -v /etc/mediabbs/oauth2.pub:/app/pubkey.pem:ro \
  -v mediabbs-uploads:/app/uploads \
  -p 127.0.0.1:8080:8080 \
  mediabbs
```

이미지 기본값: `BIND_ADDRESS=0.0.0.0:8080`, `OAUTH_PUBLIC_KEY_PATH=/app/pubkey.pem`,
`LOCAL_UPLOAD_DIR=/app/uploads`. 다른 명령은 이미지 이름 뒤에 붙여 실행합니다.

```sh
docker run --rm --env-file /etc/mediabbs/mediabbs.env mediabbs migrate
docker run --rm --env-file /etc/mediabbs/mediabbs.env mediabbs board list
```

> **주의**: 컨테이너 안에서는 `localhost`가 호스트가 아닙니다. 호스트의 MySQL에 접속하려면
> `--network host`를 쓰거나 `DATABASE_URL`에 호스트의 Docker 브리지 주소를 쓰세요.
> 또 호스트의 nginx를 거쳐 들어오는 요청은 Docker 게이트웨이 주소에서 오므로, 실제 사용자 IP를
> 얻으려면 `TRUST_PROXY`에 그 주소(예: `172.17.0.1`)를 넣어야 합니다.

### 4.2 방법 2: 직접 빌드

```sh
# Debian/Ubuntu
sudo apt install build-essential pkg-config libssl-dev default-libmysqlclient-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

git clone https://github.com/Lastorder-DC/mediabbs.git
cd mediabbs
cargo build --release
sudo useradd --system --home /opt/mediabbs mediabbs
sudo install -D target/release/mediabbs /opt/mediabbs/mediabbs
```

실행 서버에는 `libmysqlclient`(또는 `libmariadb3`)와 `libssl` 런타임만 있으면 됩니다.

### 4.3 설정 파일

[`.env.example`](../.env.example)을 복사해 고칩니다. 모든 항목의 설명은
[설정 레퍼런스](configuration.md)에 있습니다.

| 설치 방법 | 설정 파일 위치 |
|---|---|
| 직접 빌드 | **`/opt/mediabbs/.env`** — 서버와 CLI 명령 모두 작업 디렉터리의 `.env`를 자동으로 읽습니다 |
| Docker | 아무 곳 (예: `/etc/mediabbs/mediabbs.env`) — `--env-file`로 넘깁니다 |

구성 A 기준 최소 예시:

```ini
BIND_ADDRESS=127.0.0.1:8080
DATABASE_URL=mysql://mediabbs:CHANGE_ME@localhost/mediabbs

MW_BASE_URL=https://wiki.example.org
MW_SCRIPT_PATH=/w

OAUTH_CLIENT_ID=2.5에서 받은 클라이언트 키
OAUTH_CLIENT_SECRET=2.5에서 받은 비밀 키
OAUTH_PUBLIC_KEY_PATH=oauth2.pub

CORS_ALLOWED_ORIGINS=https://bbs.example.org
ADMIN_GROUPS=sysop

# 업로드: 처음에는 끄고 시작해도 됩니다 (설정 레퍼런스의 '업로드' 참고)
UPLOAD_ENABLED=false
```

위키의 공개키(2.2의 `oauth2.pub`)를 `OAUTH_PUBLIC_KEY_PATH` 위치(상대 경로는 작업 디렉터리 기준,
Docker에서는 `/app/pubkey.pem`으로 마운트)에 복사합니다. 공개키는 비밀이 아니지만,
**설정 파일에는 비밀 키와 DB 비밀번호가 있으므로** 권한을 제한하세요.

```sh
sudo cp /etc/mediawiki/oauth2.pub /opt/mediabbs/oauth2.pub   # 위키와 다른 서버라면 내용을 복사
sudo chown mediabbs: /opt/mediabbs/.env && sudo chmod 600 /opt/mediabbs/.env
```

> Docker에서는 위 예시의 `BIND_ADDRESS`와 `OAUTH_PUBLIC_KEY_PATH`를 **빼세요.** 이미지 기본값
> (`0.0.0.0:8080`, `/app/pubkey.pem`)을 그대로 쓰고, 공개키는 `-v ...:/app/pubkey.pem:ro`로 마운트합니다.

`"`나 `$`가 들어간 값(JSON, 일부 비밀번호)은 작은따옴표로 감싸세요(`KEY='값'`). 자세한 내용은
[설정 레퍼런스](configuration.md) 첫 부분을 보세요.

`MW_SCRIPT_PATH`는 위키의 `$wgScriptPath`입니다. `https://wiki.example.org/w/api.php`가 열리면 `/w`,
`https://wiki.example.org/api.php`가 열리면 빈 값입니다.

설정이 잘못되면 백엔드는 시작하자마자 `Configuration error: ...`를 출력하고 종료합니다.

### 4.4 DB 마이그레이션과 게시판 만들기

```sh
cd /opt/mediabbs
sudo -u mediabbs ./mediabbs migrate                     # 테이블 생성 (여러 번 실행해도 안전)
sudo -u mediabbs ./mediabbs board add free 자유게시판    # 이름(URL에 쓰임)  표시 이름
sudo -u mediabbs ./mediabbs board list
```

Docker라면 `docker run --rm --env-file /etc/mediabbs/mediabbs.env mediabbs migrate`처럼 실행합니다.

새로 설치하면 게시판이 하나도 없습니다. 게시판 이름은 주소(`https://bbs.example.org/free`)에
쓰이므로 소문자 영문·숫자·`-`·`_`만 쓸 수 있습니다. 자세한 관리 방법은 [운영 안내](administration.md)를 보세요.

`MIGRATE_ON_START=true`를 설정하면 서버가 시작할 때 마이그레이션을 자동으로 적용합니다.

### 4.5 서비스로 등록 (직접 빌드)

```ini
# /etc/systemd/system/mediabbs.service
[Unit]
Description=MediaBBS backend
After=network-online.target mariadb.service
Wants=network-online.target

[Service]
User=mediabbs
Group=mediabbs
# 설정은 WorkingDirectory의 .env에서 읽습니다.
WorkingDirectory=/opt/mediabbs
ExecStart=/opt/mediabbs/mediabbs serve
Restart=on-failure
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/opt/mediabbs/uploads

[Install]
WantedBy=multi-user.target
```

```sh
sudo mkdir -p /opt/mediabbs/uploads && sudo chown mediabbs: /opt/mediabbs/uploads
sudo systemctl daemon-reload
sudo systemctl enable --now mediabbs
journalctl -u mediabbs -f
```

로컬 업로드를 쓰지 않으면 `ReadWritePaths` 줄은 지워도 됩니다.

---

## 5. 클라이언트 배포

클라이언트는 빌드된 정적 파일입니다. **백엔드 주소만 알려 주면** 나머지 설정(위키 주소,
OAuth 정보, 메뉴 등)은 백엔드의 `GET /config`에서 받아 옵니다.

자세한 내용은 [mediabbs-client README](https://github.com/Lastorder-DC/mediabbs-client#readme)를 보세요.

### 5.1 방법 1: 정적 파일

```sh
git clone https://github.com/Lastorder-DC/mediabbs-client.git
cd mediabbs-client
yarn install                     # Node.js 20.19 이상
yarn build                       # 결과물: dist/
```

`dist/config.js`에 백엔드 주소를 적습니다. 기본 스킨과 테마 색도 여기서 정할 수 있습니다.

```js
window.MEDIABBS_CONFIG = {
  backendHost: "https://bbs.example.org/api",
  // skin: "liberty",          // 기본 스킨 (simple, liberty. 기본 simple)
  // menus: { simple: [{ type: "boards" }, { type: "wiki" }] },  // 스킨별 상단 메뉴
  // primaryColor: "#4188f1",  // 테마 색
};
```

`dist/`를 웹 서버 문서 루트(예: `/var/www/mediabbs-client`)에 복사합니다.

### 5.2 방법 2: Docker

```sh
cd mediabbs-client
docker build -t mediabbs-client .
docker run -d --name mediabbs-client --restart unless-stopped \
  -e BACKEND_HOST=https://bbs.example.org/api \
  -p 127.0.0.1:8081:80 \
  mediabbs-client
```

컨테이너가 시작할 때 `BACKEND_HOST`로 `config.js`를 만듭니다. 기본 스킨(`SKIN`), 고를 수 있는 스킨
목록(`SKINS`), 테마 색(`PRIMARY_COLOR`), 스킨별 상단 메뉴(`MENUS`)도 환경 변수로 줄 수 있습니다.

---

## 6. 리버스 프록시(nginx)

### 구성 A: 화면과 API를 한 주소로

```nginx
server {
    listen 443 ssl http2;
    server_name bbs.example.org;
    # ssl_certificate ... ;

    client_max_body_size 20m;          # 업로드 (UPLOAD_MAX_BYTES의 약 4/3 이상)

    # API: /api/... -> 백엔드의 /...
    location /api/ {
        proxy_pass http://127.0.0.1:8080/;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # 클라이언트 (정적 파일)
    root /var/www/mediabbs-client;
    location = /config.js         { add_header Cache-Control "no-cache"; }
    location = /index.html        { add_header Cache-Control "no-cache"; }
    location = /service-worker.js { add_header Cache-Control "no-cache"; }
    location /assets/ {
        add_header Cache-Control "public, max-age=31536000, immutable";
    }
    location / {
        try_files $uri $uri/ /index.html;     # 게시판·주제 주소는 index.html로
    }
}
```

클라이언트를 Docker로 띄웠다면 정적 파일 부분 대신 `location / { proxy_pass http://127.0.0.1:8081; }`를 씁니다.

이 구성에서의 백엔드 설정:

- `CORS_ALLOWED_ORIGINS=https://bbs.example.org` (같은 주소라 사실상 쓰이지 않지만 명시 권장)
- 로컬 업로드를 쓴다면 `UPLOAD_PUBLIC_URL=https://bbs.example.org/api/uploads/`
- nginx가 같은 서버에 있으면 `TRUST_PROXY`는 기본값(loopback)으로 충분합니다.

### 구성 B: API를 별도 서브도메인으로

```nginx
server {
    listen 443 ssl http2;
    server_name bbs-api.example.org;
    client_max_body_size 20m;
    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

- 백엔드: `CORS_ALLOWED_ORIGINS=https://bbs.example.org`
- 클라이언트: `backendHost: "https://bbs-api.example.org"`
- 위키의 OAuth 콜백 URL은 여전히 **클라이언트** 주소(`https://bbs.example.org/auth`)입니다.

### 실시간 의견 갱신 (SSE)

주제 화면은 `GET /topics/{id}/events`(Server-Sent Events)로 새 의견, 의견 숨김, 주제 상태 변경을 바로
받습니다. 위 nginx 예시 그대로 동작하며 따로 설정할 것은 없습니다.

- 백엔드가 `X-Accel-Buffering: no`를 보내므로 nginx가 이 응답만 버퍼링하지 않습니다.
- 25초마다 빈 신호를 보내므로 nginx `proxy_read_timeout` 기본값(60초)이나 Cloudflare(100초)에 끊기지 않습니다.
  `proxy_read_timeout`을 25초보다 짧게 줄였다면 늘려 주세요.
- 연결이 안 되는 환경(버퍼링하는 프록시, 동시 연결 한도 `LIVE_UPDATES_MAX_STREAMS` 초과 등)에서는
  클라이언트가 자동으로 예전 방식(목록 끝에서 10초마다 확인)으로 돌아갑니다.
- 이벤트는 백엔드 프로세스 안에서만 전달됩니다. 백엔드를 여러 대 띄우면 다른 프로세스에서 일어난 변경은
  10초 확인으로만 반영되므로, 한 대로 운영하거나 같은 주제의 요청을 한 프로세스로 보내세요.

### 실제 사용자 IP

비로그인 글쓴이 표시와 IP 차단 확인에 쓰이므로 중요합니다. 백엔드는 `TRUST_PROXY`에 등록된
프록시가 보낸 `X-Forwarded-For`/`Forwarded`만 믿습니다.

| 상황 | `TRUST_PROXY` |
|---|---|
| nginx가 같은 서버, 백엔드 직접 실행 | 기본값(비워 둠) |
| 백엔드가 Docker, nginx는 호스트 | Docker 게이트웨이 주소 또는 대역 (예: `172.17.0.1`) |
| 로드밸런서/CDN 뒤 | 그 장비들의 주소 대역 (예: `10.0.0.0/8`) |

nginx에는 위 예시처럼 `proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;`가 있어야 합니다.
글을 써 보았을 때 모든 비로그인 사용자가 같은 IP로 표시된다면 이 설정을 확인하세요.

---

## 7. 설치 확인

1. `curl https://bbs.example.org/api/config` — JSON이 나오고 `wiki.siteName`이 위키 이름이면
   백엔드와 위키 연결이 정상입니다. (`null`이면 백엔드가 위키 `api.php`에 접속하지 못하는 것입니다.)
2. `https://bbs.example.org` 접속 → 첫 게시판으로 이동하는지 확인.
3. 오른쪽 위 로그인 버튼 → 위키 로그인 → 승인 화면에서 **허용** → 게시판으로 돌아와
   아바타가 보이면 로그인 성공입니다.
4. 글쓰기 → 작성자 이름이 위키 사용자 이름인지 확인.
5. 로그아웃 후 글쓰기 → 작성자가 **본인의 실제 IP**인지 확인 (프록시 IP라면 6장의 `TRUST_PROXY` 확인).
6. 관리자 그룹 계정으로 로그인 → 주제 화면에 **관리자 메뉴**가 보이는지 확인.

문제가 있으면 [문제 해결](troubleshooting.md)을 보세요.
