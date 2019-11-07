import typing

from authlib.oauth2.rfc6749 import OAuth2Error

from .authorization_server import AuthorizationServer, RevocationEndpoint
from .resource_protector import ResourceProtector, resource_protected
from .grants import AuthorizationCodeGrant, RefreshTokenGrant

if typing.TYPE_CHECKING:
    from aiohttp.web import Application
else:
    Application = typing.Any


__all__ = ['setup', 'resource_protected', 'OAuth2Error']


def setup(app: Application):
    authorization_server = AuthorizationServer()

    # authorization_server.register_grant(grants.ImplicitGrant)
    # authorization_server.register_grant(grants.ClientCredentialsGrant)
    # authorization_server.register_grant(PasswordGrant)
    authorization_server.register_grant(AuthorizationCodeGrant)
    authorization_server.register_grant(RefreshTokenGrant)
    authorization_server.register_endpoint(RevocationEndpoint)

    protector = ResourceProtector()
    app['oauth_server'] = authorization_server
    app['resource_protector'] = protector
