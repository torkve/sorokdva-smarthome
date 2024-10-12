import sys
import time
import typing
from contextvars import ContextVar

from sqlalchemy import Integer, ForeignKey, String, create_engine
from sqlalchemy.orm import sessionmaker, relationship, DeclarativeBase, Mapped, mapped_column

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


class Base(DeclarativeBase):
    pass


class User(Base):
    __tablename__ = 'user'

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    username: Mapped[str] = mapped_column(String(40), unique=True)
    password: Mapped[str] = mapped_column(String(40))  # ahaha

    def get_user_id(self) -> int:
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


class App(Base, OAuth2ClientMixin):
    __tablename__ = 'app'

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    user_id: Mapped[int] = mapped_column(Integer, ForeignKey('user.id', ondelete='CASCADE'))
    user: Mapped[User] = relationship('User', viewonly=True)


class AuthorizationCode(Base, OAuth2AuthorizationCodeMixin):
    __tablename__ = 'oauth2_code'

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    user_id: Mapped[int] = mapped_column(Integer, ForeignKey('user.id', ondelete='CASCADE'))
    user: Mapped[User] = relationship(User)
    client_id: Mapped[str] = mapped_column(String(48), ForeignKey('app.client_id', ondelete='CASCADE'))
    client: Mapped[App] = relationship(App, viewonly=True, primaryjoin='App.client_id == AuthorizationCode.client_id')

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


class Token(Base, OAuth2TokenMixin):
    __tablename__ = 'oauth2_token'

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    user_id: Mapped[int] = mapped_column(Integer, ForeignKey('user.id', ondelete='CASCADE'))
    user: Mapped[User] = relationship('User')
    client_id: Mapped[str] = mapped_column(String(48), ForeignKey('app.client_id', ondelete='CASCADE'))
    client: Mapped[App] = relationship('App', viewonly=True, primaryjoin='App.client_id == Token.client_id')

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


class ServerSettings(Base):
    __tablename__ = 'server_settings'
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    option: Mapped[str] = mapped_column(String(), unique=True)
    value: Mapped[bytes]


db_key = web.AppKey('db', str)
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
    app[db_key] = engine
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
