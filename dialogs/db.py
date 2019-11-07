import sys
import time
import typing
from contextvars import ContextVar

from sqlalchemy_aio import ASYNCIO_STRATEGY
from sqlalchemy import Column, Integer, ForeignKey, String, Text, create_engine, select
from sqlalchemy.orm import sessionmaker, relationship
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy import schema

from aiohttp import web

# from authlib.integrations.sqla_oauth2 import OAuth2AuthorizationCodeMixin, OAuth2ClientMixin, OAuth2TokenMixin
from dialogs.authlib_integrations.models import (
    OAuth2AuthorizationCodeMixin,
    OAuth2ClientMixin,
    OAuth2TokenMixin,
)

if typing.TYPE_CHECKING:
    from sqlalchemy.orm.session import Session as OrmSession
else:
    OrmSession = typing.Any


Base = declarative_base()


class User(Base):  # type: ignore
    __tablename__ = 'user'

    id = Column(Integer, primary_key=True)
    username = Column(String(40), unique=True)
    password = Column(String(40))  # ahaha

    def get_user_id(self) -> str:
        return self.id

    def __str__(self):
        return 'User %s' % (
            ' '.join(
                f'{field}={getattr(self, field)}'
                for field in (
                    'id', 'username',
                )
            )
        )


class App(Base, OAuth2ClientMixin):  # type: ignore
    __tablename__ = 'app'

    id = Column(Integer, primary_key=True)
    user_id = Column(Integer, ForeignKey('user.id', ondelete='CASCADE'))
    user = relationship('User', viewonly=True)


class AuthorizationCode(Base, OAuth2AuthorizationCodeMixin):  # type: ignore
    __tablename__ = 'oauth2_code'

    id = Column(Integer, primary_key=True)
    user_id = Column(Integer, ForeignKey('user.id', ondelete='CASCADE'))
    user = relationship(User)
    client_id = Column(String(48), ForeignKey('app.client_id', ondelete='CASCADE'))
    client = relationship(App, viewonly=True, primaryjoin='App.client_id == AuthorizationCode.client_id')

    def __str__(self):
        return 'AuthorizationCode %s' % (
            ' '.join(
                f'{field}={getattr(self, field)}'
                for field in (
                    'id', 'user_id', 'client_id',
                    'code', 'redirect_uri', 'response_type',
                    'scope', 'auth_time',
                )
            )
        )


class Token(Base, OAuth2TokenMixin):  # type: ignore
    __tablename__ = 'oauth2_token'

    id = Column(Integer, primary_key=True)
    user_id = Column(Integer, ForeignKey('user.id', ondelete='CASCADE'))
    user = relationship('User')
    client_id = Column(String(48), ForeignKey('app.client_id', ondelete='CASCADE'))
    client = relationship('App', viewonly=True, primaryjoin='App.client_id == Token.client_id')

    def is_refresh_token_active(self):
        if self.revoked:
            return False

        expires_at = self.issued_at + self.expires_in * 2
        return expires_at >= time.time()

    def __str__(self):
        return 'Token %s' % (
            ' '.join(
                f'{field}={getattr(self, field)}'
                for field in (
                    'id', 'user_id', 'client_id',
                    'token_type', 'access_token', 'refresh_token',
                    'scope', 'revoked', 'issued_at', 'expires_in',
                )
            )
        )


class ServerSettings(Base):  # type: ignore
    __tablename__ = 'server_settings'
    id = Column(Integer, primary_key=True)
    option = Column(String(), unique=True)
    value = Column(String())


session_maker = sessionmaker()
db_session: ContextVar[OrmSession] = ContextVar('db_session')

def Session() -> OrmSession:
    return db_session.get()


def setup(app, connstring):
    engine = create_engine(
        connstring,
        # strategy=ASYNCIO_STRATEGY,
        # echo=True,
    )
    Base.metadata.create_all(engine)
    app['db'] = engine
    session_maker.configure(bind=engine)

    @web.middleware
    async def middleware(request, handler):
        session = session_maker()
        db_session.set(session)
        try:
            return await handler(request)
        finally:
            if sys.exc_info()[0] is not None:
                session.rollback()
            else:
                session.commit()

    app.middlewares.append(middleware)
