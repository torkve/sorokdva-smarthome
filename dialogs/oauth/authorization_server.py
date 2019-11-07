import json
import typing
import importlib

from aiohttp import web

from authlib.common.security import generate_token
from authlib.oauth2 import AuthorizationServer as _AuthorizationServer
from authlib.oauth2 import ClientAuthentication, OAuth2Request
from authlib.oauth2.rfc6749 import OAuth2Error, InvalidGrantError
from authlib.oauth2.rfc6750 import BearerToken
from authlib.oauth2.rfc7009 import RevocationEndpoint as _RevocationEndpoint

from dialogs.db import User, App, Token, Session


class AuthorizationServer(_AuthorizationServer):
    def __init__(
        self,
        config: typing.Optional[dict] = None,
        error_uris: typing.Optional[str] = None,
    ):
        self.config = config.copy() if config is not None else {}
        self.config.setdefault('error_uris', error_uris)

        super().__init__(
            query_client=query_client,
            save_token=save_token,
            generate_token=self._create_bearer_token_generator(),
        )
        self.authentication_client = ClientAuthentication(self.query_client)

    def get_error_uris(self, request) -> typing.Optional[dict]:
        error_uris = self.config.get('error_uris')
        return dict(error_uris) if error_uris else None

    async def create_oauth2_request(self, request: web.Request) -> OAuth2Request:
        if isinstance(request, OAuth2Request):
            return request

        if request.method == 'POST':
            body: typing.Any = await request.post()
        else:
            body = None

        return OAuth2Request(
            request.method,
            str(request.rel_url),
            body,
            request.headers,
        )

    async def create_endpoint_response(  # type: ignore
        self,
        name: str,
        request: web.Request
    ) -> web.Response:
        if name not in self._endpoints:
            raise RuntimeError(f'There is no {name!r} endpoint.')

        oauth_request = await self.create_oauth2_request(request)
        endpoint_cls = self._endpoints[name]
        endpoint = endpoint_cls(oauth_request, self)
        return self.handle_response(*endpoint())

    async def create_authorization_response(  # type: ignore
        self,
        request: web.Request,
        grant_user: typing.Optional[User] = None,
    ) -> web.Response:
        oauth_request = await self.create_oauth2_request(request)
        try:
            grant = self.get_authorization_grant(oauth_request)
        except InvalidGrantError as error:
            return self.handle_error_response(oauth_request, error)

        try:
            redirect_uri = grant.validate_authorization_request()
            args = grant.create_authorization_response(redirect_uri, grant_user)
            return self.handle_response(*args)
        except OAuth2Error as error:
            return self.handle_error_response(oauth_request, error)

    async def create_token_response(  # type: ignore
        self,
        request: web.Request
    ) -> web.Response:
        oauth_request = await self.create_oauth2_request(request)
        try:
            grant = self.get_token_grant(oauth_request)
        except InvalidGrantError as error:
            return self.handle_error_response(oauth_request, error)

        try:
            grant.validate_token_request()
            args = grant.create_token_response()
            return self.handle_response(*args)
        except OAuth2Error as error:
            return self.handle_error_response(oauth_request, error)

    def handle_response(self, status_code: int, payload: typing.Any, headers: dict) -> web.Response:
        if isinstance(payload, dict):
            return web.Response(
                text=json.dumps(payload),
                status=status_code,
                headers=headers,
            )
        else:
            return web.Response(
                text=payload,
                status=status_code,
                headers=headers,
            )

    async def validate_consent_request(self, request: web.Request, end_user: User):
        oauth_request = await self.create_oauth2_request(request)
        oauth_request.user = end_user

        grant = self.get_authorization_grant(oauth_request)
        grant.validate_consent_request()
        if not hasattr(grant, 'prompt'):
            grant.prompt = None
        return grant

    def _create_bearer_token_generator(self) -> BearerToken:
        access_token_generator = create_token_generator(
            self.config.get('ACCESS_TOKEN_GENERATOR', True),
            42,
        )

        refresh_token_generator = create_token_generator(
            self.config.get('REFRESH_TOKEN_GENERATOR', True),
            48
        )

        expires_generator = create_token_expires_in_generator(
            # {
            #    'authorization_code': 864000,
            #    'urn:ietf:params:oauth:grant-type:jwt-bearer': 3600,
            # }
            self.config.get('TOKEN_EXPIRES_IN')
        )

        return BearerToken(access_token_generator, refresh_token_generator, expires_generator)


def query_client(client_id) -> typing.Optional[App]:
    return Session().query(App).filter_by(client_id=client_id).first()


def save_token(token, request):
    user_id = request.user.get_user_id() if request.user else None
    client = request.client
    item = Token(
        client_id=client.client_id,
        user_id=user_id,
        **token
    )
    session = Session()
    session.add(item)
    session.commit()


class RevocationEndpoint(_RevocationEndpoint):
    CLIENT_AUTH_METHODS = ['client_secret_post']

    def query_token(self, token: str, token_type_hint: str, client: App) -> typing.Optional[Token]:
        query = Session().query(Token).filter_by(client_id=client.client_id, revoked=False)
        if token_type_hint == 'access_token':
            return query.filter_by(access_token=token).first()
        elif token_type_hint == 'refresh_token':
            return query.filter_by(refresh_token=token).first()

        item = query.filter_by(access_token=token).first()
        if item:
            return item
        return query.filter_by(refresh_token=token).first()

    def revoke_token(self, token: Token):
        token.revoked = True  # type: ignore
        session = Session()
        session.add(token)
        session.commit()


@typing.overload
def create_token_generator(cfg: str, length: int) -> typing.Callable[..., str]: ...
@typing.overload
def create_token_generator(cfg: typing.Callable[..., str], length: int) -> typing.Callable[..., str]: ...
@typing.overload
def create_token_generator(cfg: bool, length: int) -> typing.Optional[typing.Callable[..., str]]: ...
def create_token_generator(cfg, length = 42):
    if callable(cfg):
        return cfg
    elif isinstance(cfg, str):
        module, obj = cfg.split(':')
        return getattr(importlib.import_module(module), obj)
    elif cfg is True:
        return lambda *args, length=length, **kwargs: generate_token(length)
    else:
        return None


def create_token_expires_in_generator(cfg: typing.Optional[dict]) -> typing.Callable[..., int]:
    data = BearerToken.GRANT_TYPES_EXPIRES_IN.copy()
    if cfg:
        data.update(cfg)

    return lambda client, grant_type, data=data: data.get(grant_type, BearerToken.DEFAULT_EXPIRES_IN)
