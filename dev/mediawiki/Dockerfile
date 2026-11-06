# MediaWiki 1.43 LTS + Extension:OAuth, preconfigured for local mediabbs testing.
# NOT for production use.
FROM mediawiki:1.43

COPY --from=composer:2 /usr/bin/composer /usr/bin/composer
RUN apt-get update \
    && apt-get install -y --no-install-recommends unzip \
    && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 -b REL1_43 \
        https://gerrit.wikimedia.org/r/mediawiki/extensions/OAuth extensions/OAuth \
    && cd extensions/OAuth \
    && COMPOSER_ALLOW_SUPERUSER=1 composer install --no-dev --no-interaction --no-progress \
    && rm -rf .git

COPY createOAuth2Consumer.php /var/www/html/extensions/OAuth/maintenance/createOAuth2Consumer.php
COPY setup.sh /usr/local/bin/mediabbs-wiki-setup
RUN chmod +x /usr/local/bin/mediabbs-wiki-setup

ENTRYPOINT ["/usr/local/bin/mediabbs-wiki-setup"]
CMD ["apache2-foreground"]
