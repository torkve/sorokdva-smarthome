import time
import json

from sqlalchemy import String, Boolean, Text, Integer
from sqlalchemy.orm import mapped_column, Mapped
from sqlalchemy.ext.hybrid import hybrid_property

from authlib.oauth2.rfc6749 import ClientMixin, TokenMixin, AuthorizationCodeMixin
from authlib.oauth2.rfc6749.util import scope_to_list, list_to_scope


class OAuth2ClientMixin(ClientMixin):
    client_id: Mapped[str] = mapped_column(String(48), index=True)
    client_secret: Mapped[str] = mapped_column(String(120))
    issued_at: Mapped[int] = mapped_column(
        Integer, nullable=False,
        default=lambda: int(time.time())
    )
    expires_at: Mapped[int] = mapped_column(Integer, nullable=False, default=0)

    redirect_uri: Mapped[str] = mapped_column(Text)
    token_endpoint_auth_method: Mapped[str] = mapped_column(
        String(48), default='client_secret_basic')
    grant_type: Mapped[str] = mapped_column(Text, nullable=False, default='')
    response_type: Mapped[str] = mapped_column(Text, nullable=False, default='')
    scope: Mapped[str] = mapped_column(Text, nullable=False, default='')

    client_name: Mapped[str] = mapped_column(String(100))
    client_uri: Mapped[str] = mapped_column(Text)
    logo_uri: Mapped[str] = mapped_column(Text, nullable=True)
    contact: Mapped[str] = mapped_column(Text, nullable=True)
    tos_uri: Mapped[str] = mapped_column(Text, nullable=True)
    policy_uri: Mapped[str] = mapped_column(Text, nullable=True)
    jwks_uri: Mapped[str] = mapped_column(Text, nullable=True)
    jwks_text: Mapped[str] = mapped_column(Text, nullable=True)
    i18n_metadata: Mapped[str] = mapped_column(Text, nullable=True)

    software_id: Mapped[str] = mapped_column(String(36), nullable=True)
    software_version: Mapped[str] = mapped_column(String(48), nullable=True)

    def __repr__(self):
        return '<Client: {}>'.format(self.client_id)

    @hybrid_property
    def redirect_uris(self):
        if self.redirect_uri:
            return self.redirect_uri.splitlines()
        return []

    @redirect_uris.setter  # type: ignore
    def redirect_uris(self, value):
        self.redirect_uri = '\n'.join(value)

    @hybrid_property
    def grant_types(self):
        if self.grant_type:
            return self.grant_type.splitlines()
        return []

    @grant_types.setter  # type: ignore
    def grant_types(self, value):
        self.grant_type = '\n'.join(value)

    @hybrid_property
    def response_types(self):
        if self.response_type:
            return self.response_type.splitlines()
        return []

    @response_types.setter  # type: ignore
    def response_types(self, value):
        self.response_type = '\n'.join(value)

    @hybrid_property
    def contacts(self):
        if self.contact:
            return json.loads(self.contact)
        return []

    @contacts.setter  # type: ignore
    def contacts(self, value):
        self.contact = json.dumps(value)

    @hybrid_property
    def jwks(self):
        if self.jwks_text:
            return json.loads(self.jwks_text)
        return None

    @jwks.setter  # type: ignore
    def jwks(self, value):
        self.jwks_text = json.dumps(value)

    @hybrid_property
    def client_metadata(self):
        """Implementation for Client Metadata in OAuth 2.0 Dynamic Client
        Registration Protocol via `Section 2`_.

        .. _`Section 2`: https://tools.ietf.org/html/rfc7591#section-2
        """
        keys = [
            'redirect_uris', 'token_endpoint_auth_method', 'grant_types',
            'response_types', 'client_name', 'client_uri', 'logo_uri',
            'scope', 'contacts', 'tos_uri', 'policy_uri', 'jwks_uri', 'jwks',
        ]
        metadata = {k: getattr(self, k) for k in keys}
        if self.i18n_metadata:
            metadata.update(json.loads(self.i18n_metadata))
        return metadata

    @client_metadata.setter  # type: ignore
    def client_metadata(self, value):
        i18n_metadata = {}
        for k in value:
            if hasattr(self, k):
                setattr(self, k, value[k])
            elif '#' in k:
                i18n_metadata[k] = value[k]

        self.i18n_metadata = json.dumps(i18n_metadata)

    @property
    def client_info(self):
        """Implementation for Client Info in OAuth 2.0 Dynamic Client
        Registration Protocol via `Section 3.2.1`_.

        .. _`Section 3.2.1`: https://tools.ietf.org/html/rfc7591#section-3.2.1
        """
        return dict(
            client_id=self.client_id,
            client_secret=self.client_secret,
            client_id_issued_at=self.issued_at,
            client_secret_expires_at=self.expires_at,
        )

    def get_client_id(self) -> str:
        return self.client_id

    def get_default_redirect_uri(self) -> str | None:
        if self.redirect_uris:
            return self.redirect_uris[0]
        return None

    def get_allowed_scope(self, scope) -> str:
        if not scope:
            return ''
        allowed = set(self.scope.split())
        scopes = scope_to_list(scope)
        return list_to_scope([s for s in scopes if s in allowed])

    def check_redirect_uri(self, redirect_uri) -> bool:
        return redirect_uri in self.redirect_uris

    def check_client_secret(self, client_secret) -> bool:
        return self.client_secret == client_secret

    def check_endpoint_auth_method(self, method, endpoint) -> bool:
        return self.token_endpoint_auth_method == method

    def check_response_type(self, response_type) -> bool:
        if self.response_type:
            return response_type in self.response_types
        return False

    def check_grant_type(self, grant_type) -> bool:
        if self.grant_type:
            return grant_type in self.grant_types
        return False


class OAuth2AuthorizationCodeMixin(AuthorizationCodeMixin):
    code: Mapped[str] = mapped_column(String(120), unique=True, nullable=False)
    client_id: Mapped[str] = mapped_column(String(48))
    redirect_uri: Mapped[str] = mapped_column(Text, default='')
    response_type: Mapped[str] = mapped_column(Text, default='')
    scope: Mapped[str] = mapped_column(Text, default='')
    auth_time: Mapped[int] = mapped_column(
        Integer, nullable=False,
        default=lambda: int(time.time())
    )

    def is_expired(self):
        return self.auth_time + 300 < time.time()

    def get_redirect_uri(self):
        return self.redirect_uri

    def get_scope(self):
        return self.scope

    def get_auth_time(self):
        return self.auth_time


class OAuth2TokenMixin(TokenMixin):
    client_id: Mapped[str] = mapped_column(String(48))
    token_type: Mapped[str] = mapped_column(String(40))
    access_token: Mapped[str] = mapped_column(String(255), unique=True, nullable=False)
    refresh_token: Mapped[str] = mapped_column(String(255), index=True, nullable=True)
    scope: Mapped[str] = mapped_column(Text, default='')
    revoked: Mapped[bool] = mapped_column(Boolean, default=False)
    issued_at: Mapped[int] = mapped_column(
        Integer, nullable=False, default=lambda: int(time.time())
    )
    expires_in: Mapped[int] = mapped_column(Integer, nullable=False, default=0)

    def check_client(self, client) -> bool:
        return self.client_id == client.client_id

    def get_scope(self) -> str:
        return self.scope

    def get_expires_in(self) -> int:
        return self.expires_in

    def is_expired(self) -> bool:
        return self.issued_at + self.expires_in < time.time()

    def is_revoked(self) -> bool:
        return self.revoked
