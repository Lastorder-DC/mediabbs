<?php
/**
 * Creates (and optionally approves) an OAuth 2.0 consumer.
 * The bundled createOAuthConsumer.php only creates OAuth 1.0a consumers.
 *
 * Usage:
 *   php extensions/OAuth/maintenance/createOAuth2Consumer.php --user Admin \
 *     --name mediabbs --callbackUrl https://bbs.example.org/auth \
 *     --grants authonlyprivate --approve
 *
 * Prints JSON: {"key": "<client id>", "secret": "<client secret>", ...}
 */

namespace MediaWiki\Extension\OAuth;

use MediaWiki\Context\RequestContext;
use MediaWiki\Extension\OAuth\Backend\Consumer;
use MediaWiki\Extension\OAuth\Backend\Utils;
use MediaWiki\Extension\OAuth\Control\ConsumerSubmitControl;
use MediaWiki\Maintenance\Maintenance;
use MediaWiki\User\User;
use MWRestrictions;

$IP = getenv( 'MW_INSTALL_PATH' );
if ( $IP === false ) {
	$IP = __DIR__ . '/../../..';
}
require_once "$IP/maintenance/Maintenance.php";

class CreateOAuth2Consumer extends Maintenance {
	public function __construct() {
		parent::__construct();
		$this->addDescription( 'Create an OAuth 2.0 consumer' );
		$this->addOption( 'user', 'Owner (must be registered and have an email)', true, true );
		$this->addOption( 'name', 'Application name', true, true );
		$this->addOption( 'description', 'Application description', false, true );
		$this->addOption( 'version', 'Application version', false, true );
		$this->addOption( 'callbackUrl', 'Callback (redirect) URL', true, true );
		$this->addOption( 'grants', 'authonly (identity only), authonlyprivate (identity + ' .
			'real name/email), or grant names such as basic, editpage', true, true, false, true );
		$this->addOption( 'approve', 'Approve the consumer immediately' );
		$this->requireExtension( 'OAuth' );
	}

	public function execute() {
		$user = User::newFromName( $this->getOption( 'user' ) );
		if ( !$user || !$user->isNamed() ) {
			$this->fatalError( 'User must be registered' );
		}
		if ( $user->getEmail() === '' ) {
			$this->fatalError( 'User must have an email' );
		}
		$grants = $this->getOption( 'grants' );
		$authOnly = array_values( array_intersect( $grants, [ 'authonly', 'authonlyprivate' ] ) );

		$data = [
			'action' => 'propose',
			'name' => $this->getOption( 'name' ),
			'version' => $this->getOption( 'version', '1.0' ),
			'description' => $this->getOption( 'description', 'mediabbs' ),
			'callbackUrl' => $this->getOption( 'callbackUrl' ),
			'oauthVersion' => Consumer::OAUTH_VERSION_2,
			'callbackIsPrefix' => false,
			'grants' => json_encode( $authOnly ? [] : array_values( $grants ) ),
			'granttype' => $authOnly ? $authOnly[0] : 'normal',
			'ownerOnly' => false,
			'oauth2IsConfidential' => true,
			'oauth2GrantTypes' => [ 'authorization_code', 'refresh_token' ],
			'email' => $user->getEmail(),
			'wiki' => '*',
			'rsaKey' => '',
			'agreement' => true,
			'restrictions' => MWRestrictions::newDefault(),
		];

		$context = RequestContext::getMain();
		$context->setUser( $user );
		$dbw = Utils::getCentralDB( DB_PRIMARY );
		$status = ( new ConsumerSubmitControl( $context, $data, $dbw ) )->submit();
		if ( !$status->isGood() ) {
			$this->fatalError( $status->getMessage()->text() );
		}
		/** @var Consumer $cmr */
		$cmr = $status->value['result']['consumer'];

		$approved = null;
		if ( $this->hasOption( 'approve' ) ) {
			$approveStatus = ( new ConsumerSubmitControl( $context, [
				'action' => 'approve',
				'consumerKey' => $cmr->getConsumerKey(),
				'reason' => 'Approved by maintenance script',
				'changeToken' => $cmr->getChangeToken( $context ),
			], $dbw ) )->submit();
			$approved = $approveStatus->isGood();
		}

		$this->output( json_encode( [
			'key' => $cmr->getConsumerKey(),
			'secret' => Utils::hmacDBSecret( $cmr->getSecretKey() ),
			'approved' => $approved,
		] ) . "\n" );
	}
}

$maintClass = CreateOAuth2Consumer::class;
require_once RUN_MAINTENANCE_IF_MAIN;
