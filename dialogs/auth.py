import typing

import aiohttp_session
import aiohttp_security
from aiohttp import web

from dialogs import db


__all__ = ['setup']


# NOTE (torkve) aiohttp_security assumes that identity policy should somehow
# identify user based on request, and return 'str' identity which would be
# used by auth policy to find user ID.
# However auth policy has no context (e.g. request) in their world,
# so we intentionally violate their type system here: we resolve 'User'
# instead of 'str' in identify, and it is sufficient for us
# in authorized_userid to find user ID from it.
class SimpleAuthPolicy(aiohttp_security.AbstractAuthorizationPolicy):
    async def permits(self, identity, permission, context=None):
        return True

    async def authorized_userid(self, identity: db.User) -> str:  # type: ignore
        return str(identity.id)


class SessionIdentityPolicy(aiohttp_security.AbstractIdentityPolicy):
    def __init__(self, session_key='AIOHTTP_SECURITY'):
        self._session_key = session_key

    async def identify(self, request: web.Request) -> typing.Optional[db.User]:  # type: ignore
        session = await aiohttp_session.get_session(request)
        try:
            user_id_str = session.get(self._session_key)
            assert user_id_str is None or isinstance(user_id_str, str)
            user_id = int(user_id_str or "")
        except (TypeError, ValueError):
            return None

        if user_id is not None:
            return db.Session().get(db.User, user_id)
        else:
            return None

    async def remember(
        self,
        request: web.Request,
        response: web.StreamResponse,
        identity: str,
        **kwargs
    ):
        session = await aiohttp_session.get_session(request)
        session[self._session_key] = identity

    async def forget(self, request: web.Request, response: web.StreamResponse):
        session = await aiohttp_session.get_session(request)
        session.pop(self._session_key, None)


def setup(app):
    aiohttp_security.setup(app, SessionIdentityPolicy(), SimpleAuthPolicy())
