import json
import typing
import functools

from aiohttp import web

from authlib.oauth2 import OAuth2Error, ResourceProtector as _ResourceProtector
from authlib.oauth2.rfc6749 import OAuth2Request, MissingAuthorizationError
from authlib.oauth2.rfc6749.util import scope_to_list
from authlib.oauth2.rfc6750 import BearerTokenValidator as _BearerTokenValidator

from dialogs.db import Session, Token


class JSONException(web.HTTPException):
    def __init__(self, status_code: int, data: typing.Any, headers: typing.Optional[typing.Any]):
        body = json.dumps(data)
        web.Response.__init__(
            self,
            status=status_code,
            headers=headers,
            text=body,
        )
        Exception.__init__(self, body)


class BearerTokenValidator(_BearerTokenValidator):
    def authenticate_token(self, token_string: str) -> typing.Optional[Token]:
        return Session().query(Token).filter_by(access_token=token_string).first()

    def request_invalid(self, request):
        return False

    def token_revoked(self, token: Token):
        return token.revoked


class ResourceProtector(_ResourceProtector):
    def __init__(self):
        super().__init__()
        self.register_token_validator(BearerTokenValidator())

    def raise_error_response(self, error: OAuth2Error) -> typing.NoReturn:
        status_code = error.status_code
        data = dict(error.get_body())
        headers = error.get_headers()
        raise JSONException(status_code, data, headers)

    async def acquire_token(self, request: web.Request, scopes: list[str] | str | None = None):
        token_request = OAuth2Request(
            request.method,
            request.path,
            await request.text(),
            request.headers,
        )

        kwargs = {}
        scopes = scope_to_list(scopes)
        if scopes:
            kwargs['scopes'] = scopes

        token = self.validate_request(request=token_request, **kwargs)
        request['oauth_token'] = token
        return token


protector_key = web.AppKey('resource_protector', ResourceProtector)


def resource_protected(scopes: list[str] | str | None = None, optional=False):
    def wrapper(f):
        @functools.wraps(f)
        async def handler(request: web.Request):
            request['oauth_token'] = None
            protector = request.app[protector_key]

            try:
                token = await protector.acquire_token(request, scopes)
            except MissingAuthorizationError as error:
                if not optional:
                    protector.raise_error_response(error)
            except OAuth2Error as error:
                protector.raise_error_response(error)
            else:
                request['oauth_token'] = token

            return await f(request)

        return handler

    return wrapper
