import typing

import aiohttp_session
import aiohttp_security
from aiohttp import web

from dialogs import db


__all__ = ['setup']


class SimpleAuthPolicy(aiohttp_security.AbstractAuthorizationPolicy):
    async def permits(self, identity, permission, context=None):
        return True

    async def authorized_userid(self, identity: db.User) -> int:
        return identity.id


class SessionIdentityPolicy(aiohttp_security.AbstractIdentityPolicy):
    def __init__(self, session_key='AIOHTTP_SECURITY'):
        self._session_key = session_key

    async def identify(self, request: web.Request) -> typing.Optional[db.User]:
        session = await aiohttp_session.get_session(request)
        try:
            user_id = int(session.get(self._session_key))
        except (TypeError, ValueError):
            return None

        if user_id is not None:
            return db.Session().query(db.User).get(user_id)
        else:
            return None

    async def remember(
        self,
        request: web.Request,
        response: web.Response,
        identity: int,  # aiohttp_security is broken here and asserts for not identity type, but userid
        **kwargs
    ):
        session = await aiohttp_session.get_session(request)
        session[self._session_key] = identity

    async def forget(self, request: web.Request, response: web.Response):
        session = await aiohttp_session.get_session(request)
        session.pop(self._session_key, None)


def setup(app):
    aiohttp_security.setup(app, SessionIdentityPolicy(), SimpleAuthPolicy())

