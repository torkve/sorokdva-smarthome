import typing

from authlib.oauth2.rfc6749 import OAuth2Error

from .authorization_server import AuthorizationServer, RevocationEndpoint, server_key
from .resource_protector import ResourceProtector, resource_protected, protector_key
from .grants import AuthorizationCodeGrant, RefreshTokenGrant

if typing.TYPE_CHECKING:
    from aiohttp.web import Application
else:
    Application = typing.Any


__all__ = ['setup', 'resource_protected', 'OAuth2Error', 'server_key', 'protector_key']


def setup(app: Application):
    authorization_server = AuthorizationServer()

    # authorization_server.register_grant(grants.ImplicitGrant)
    # authorization_server.register_grant(grants.ClientCredentialsGrant)
    # authorization_server.register_grant(PasswordGrant)
    authorization_server.register_grant(AuthorizationCodeGrant)
    authorization_server.register_grant(RefreshTokenGrant)
    authorization_server.register_endpoint(RevocationEndpoint)

    protector = ResourceProtector()
    app[server_key] = authorization_server
    app[protector_key] = protector
