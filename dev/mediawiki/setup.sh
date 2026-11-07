#!/bin/bash
# First-run setup of the development wiki, then exec the container command.
# Creates: SQLite wiki, OAuth 2.0 key pair, test users and an OAuth 2.0
# consumer for mediabbs. Credentials for mediabbs are written to /shared.
set -euo pipefail

DATA=/var/www/data
SHARED=/shared
WIKI_SERVER=${WIKI_SERVER:-http://127.0.0.1:18100}
WIKI_LANG=${WIKI_LANG:-ko}
BBS_CALLBACK=${BBS_CALLBACK:-http://127.0.0.1:18090/auth}
ADMIN_PASSWORD=${ADMIN_PASSWORD:-mediabbs-dev-admin}
USER_PASSWORD=${USER_PASSWORD:-mediabbs-dev-user}
WIKI_NAME=${WIKI_NAME:-MediaBBS Dev Wiki}

cd /var/www/html
mkdir -p "$DATA" "$SHARED"

# LocalSettings.php lives in the data volume; a stub includes it (a symlink is
# not followed by www-data under fs.protected_symlinks on some hosts).
# Extra settings can be mounted as /var/www/conf.d/*.php (loaded afterwards).
link_settings() {
    rm -f LocalSettings.php
    printf '<?php\nrequire "%s/LocalSettings.php";\nforeach ( glob( "/var/www/conf.d/*.php" ) ?: [] as $f ) { require $f; }\n' "$DATA" > LocalSettings.php
}

if [ ! -f "$DATA/LocalSettings.php" ]; then
    echo "== Installing MediaWiki \"$WIKI_NAME\" ($WIKI_SERVER, lang=$WIKI_LANG)"
    php maintenance/install.php \
        --dbtype sqlite --dbpath "$DATA" --dbname wiki \
        --server "$WIKI_SERVER" --scriptpath "" --lang "$WIKI_LANG" \
        --pass "$ADMIN_PASSWORD" --confpath "$DATA" \
        "$WIKI_NAME" Admin

    echo "== Generating OAuth 2.0 key pair"
    openssl genrsa -out "$DATA/oauth2.key" 2048 2>/dev/null
    openssl rsa -in "$DATA/oauth2.key" -pubout -out "$DATA/oauth2.pub" 2>/dev/null

    cat >> "$DATA/LocalSettings.php" <<'PHP'

# ---- mediabbs development settings ----
$wgArticlePath = "/wiki/$1";
$wgEnableUploads = false;
$wgEmailAuthentication = false;

wfLoadExtension( 'OAuth' );
$wgOAuth2PrivateKey = '/var/www/data/oauth2.key';
$wgOAuth2PublicKey = '/var/www/data/oauth2.pub';
# Plain HTTP is fine for local testing only.
$wgMWOAuthSecureTokenTransfer = false;
# Short-lived access tokens make the refresh flow easy to exercise.
$wgOAuth2GrantExpirationInterval = getenv( 'OAUTH2_ACCESS_TOKEN_TTL' ) ?: 'PT1H';

$wgGroupPermissions['sysop']['mwoauthproposeconsumer'] = true;
$wgGroupPermissions['sysop']['mwoauthmanageconsumer'] = true;
$wgGroupPermissions['sysop']['mwoauthupdateownconsumer'] = true;
$wgGroupPermissions['sysop']['mwoauthviewsuppressed'] = true;

# Example of a dedicated moderator group (librewiki used "boardmanager").
$wgGroupPermissions['boardmanager']['read'] = true;
PHP
    link_settings
    php maintenance/update.php --quick

    echo "== Creating test users"
    php maintenance/resetUserEmail.php --no-reset-password Admin admin@example.invalid
    php maintenance/createAndPromote.php Alice "$USER_PASSWORD"
    php maintenance/createAndPromote.php --custom-groups boardmanager Moderator "$USER_PASSWORD"
    php maintenance/createAndPromote.php Blocked "$USER_PASSWORD"
    echo "Blocked" | php maintenance/blockUsers.php --performer Admin --reason "test block"

    echo "== Creating OAuth 2.0 consumers"
    php extensions/OAuth/maintenance/createOAuth2Consumer.php --user Admin \
        --name mediabbs-dev --callbackUrl "$BBS_CALLBACK" \
        --grants authonlyprivate --approve | tee "$DATA/consumer.json"
    php extensions/OAuth/maintenance/createOAuth2Consumer.php --user Admin \
        --name mediabbs-dev-authonly --callbackUrl "$BBS_CALLBACK" \
        --grants authonly --approve | tee "$DATA/consumer-authonly.json"
fi

link_settings

# Hand the credentials to the mediabbs backend container.
php -r '
    $c = json_decode(file_get_contents("/var/www/data/consumer.json"), true);
    $a = json_decode(file_get_contents("/var/www/data/consumer-authonly.json"), true);
    file_put_contents("/shared/oauth.env",
        "OAUTH_CLIENT_ID=" . $c["key"] . "\nOAUTH_CLIENT_SECRET=" . $c["secret"] . "\n" .
        "# identity-only consumer (no email), for testing:\n" .
        "#OAUTH_CLIENT_ID=" . $a["key"] . "\n#OAUTH_CLIENT_SECRET=" . $a["secret"] . "\n");
'
cp "$DATA/oauth2.pub" "$SHARED/pubkey.pem"
chmod 0644 "$SHARED/oauth.env" "$SHARED/pubkey.pem"
chown -R www-data:www-data "$DATA"

exec docker-php-entrypoint "$@"
