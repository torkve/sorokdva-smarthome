import typing

from authlib.common.security import generate_token
from authlib.oauth2.rfc6749 import grants

from dialogs.db import Session, User, App, AuthorizationCode, Token


class AuthorizationCodeGrant(grants.AuthorizationCodeGrant):
    def create_authorization_code(self, client: App, grant_user: User, request) -> str:
        code = generate_token(48)
        item = AuthorizationCode(
            code=code,
            client_id=client.client_id,
            redirect_uri=request.redirect_uri,
            scope=request.scope,
            user_id=grant_user.id,
        )
        session = Session()
        session.add(item)
        session.commit()
        return code

    def parse_authorization_code(self, code: str, client: App) -> typing.Optional[AuthorizationCode]:
        item = Session().query(AuthorizationCode).filter_by(
            code=code,
            client_id=client.client_id
        ).first()
        if item and not item.is_expired():
            return item
        return None

    def delete_authorization_code(self, authorization_code: AuthorizationCode) -> None:
        session = Session()
        session.delete(authorization_code)
        session.commit()

    def authenticate_user(self, authorization_code: AuthorizationCode) -> typing.Optional[User]:
        return Session().query(User).get(authorization_code.user_id)


# class PasswordGrant(grants.ResourceOwnerPasswordCredentialsGrant):
#     def authenticate_user(self, username: str, password: str) -> typing.Optional[User]:
#         user = Session().query(User).filter_by(username=username).first()
#         if user is not None and user.check_password(password):
#             return user
#         return None


class RefreshTokenGrant(grants.RefreshTokenGrant):
    TOKEN_ENDPOINT_AUTH_METHODS = ['client_secret_post']

    def authenticate_refresh_token(self, refresh_token: str) -> typing.Optional[Token]:
        token = Session().query(Token).filter_by(refresh_token=refresh_token).first()
        if token and token.is_refresh_token_active():
            return token
        return None

    def authenticate_user(self, credential: Token) -> typing.Optional[User]:
        return Session().query(User).get(credential.user_id)

    def revoke_old_credential(self, credential: Token) -> None:
        credential.revoked = True  # type: ignore

        session = Session()
        session.add(credential)
        session.commit()
