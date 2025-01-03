import typing
from aiohttp import web
from authlib.oauth2 import AuthorizationServer as _AuthorizationServer, OAuth2Request, JsonRequest, OAuth2Error
from authlib.oauth2.rfc6749 import BaseGrant
from authlib.oauth2.rfc7009 import RevocationEndpoint as _RevocationEndpoint
from dialogs.db import App as App, Token as Token, User as User
from typing import Any


class AuthorizationServer(_AuthorizationServer):
    config: Any = ...
    authentication_client: Any = ...
    def __init__(self, config: typing.Optional[dict] = ..., error_uris: typing.Optional[str] = ...) -> None: ...
    def query_client(self, client_id: str) -> typing.Optional[App]: ...
    def save_token(self, token: dict, request: OAuth2Request) -> None: ...
    def get_error_uris(self, request: OAuth2Request) -> typing.Optional[dict]: ...
    def get_error_uri(self, request: OAuth2Request, error: Exception) -> OAuth2Error: ...
    async def create_oauth2_request(self, request: web.Request) -> OAuth2Request: ...
    async def create_json_request(self, request: web.Request) -> JsonRequest: ...
    async def create_endpoint_response(self, name: str, request: web.Request) -> web.Response: ...
    async def create_authorization_response(self, request: web.Request, grant_user: typing.Optional[User] = ...) -> web.Response: ...
    async def create_token_response(self, request: web.Request) -> web.Response: ...
    def handle_response(self, status_code: int, payload: typing.Any, headers: dict) -> web.Response: ...
    async def get_consent_grant(self, request: web.Request, end_user: User) -> BaseGrant: ...
    def send_signal(self, name: str, *args, **kwargs): ...


def query_client(client_id: str) -> typing.Optional[App]: ...
def save_token(token: dict, request: OAuth2Request) -> None: ...


class RevocationEndpoint(_RevocationEndpoint):
    CLIENT_AUTH_METHODS: list[str] = ...
    def authenticate_token(self, request: OAuth2Request, client: App): ...
    async def create_endpoint_request(self, request: web.Request) -> OAuth2Request: ...
    def query_token(self, token: str, token_type_hint: str, client: App) -> typing.Optional[Token]: ...
    def revoke_token(self, token: Token, request: OAuth2Request) -> None: ...


@typing.overload
def create_token_generator(cfg: str, length: int) -> typing.Callable[..., str]: ...
@typing.overload
def create_token_generator(cfg: typing.Callable[..., str], length: int) -> typing.Callable[..., str]: ...
@typing.overload
def create_token_generator(cfg: bool, length: int) -> typing.Optional[typing.Callable[..., str]]: ...
def create_token_expires_in_generator(cfg: typing.Optional[dict]) -> typing.Callable[..., int]: ...


server_key: web.AppKey[AuthorizationServer]
