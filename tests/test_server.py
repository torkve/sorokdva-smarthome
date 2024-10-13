import yarl
import asyncio
from unittest import mock

import pytest
from sqlalchemy.orm import Session
from aiohttp import ClientSession
from aiohttp.web import Application
from aiohttp.test_utils import TestClient, TestServer

from dialogs import db
from dialogs.app import make_app


pytestmark = pytest.mark.asyncio
cfg: dict = {
    'devices': {},
}


@pytest.fixture(scope='session', autouse=True)
def mock_https_check():
    with mock.patch('authlib.oauth2.rfc6749.errors.InsecureTransportError.check') as _check:
        yield _check


@pytest.fixture(scope='function')
async def app():
    app = await make_app(cfg, ':memory:')
    db_session = Session(bind=app[db.db_key])

    db_session.add(db.User(username='username', password='password'))
    db_session.commit()

    return app


@pytest.fixture(scope='function')
async def client(app):
    client = TestClient(TestServer(app))
    await client.start_server()

    try:
        yield client
    finally:
        await client.close()


async def test_no_register(client: ClientSession):
    conn = client.post('/auth/register', data={'username': 'user1', 'password': 'password'})
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 404, await resp.text()


async def test_authorize(app: Application, client: ClientSession):
    conn = client.post('/auth', data={'username': 'username', 'password': 'password'})
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()

    conn = client.post('/auth', data={'username': 'username', 'password': 'wrong'})
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 403, await resp.text()

    conn = client.post('/auth', data={'username': 'wrong', 'password': 'password'})
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 403, await resp.text()


@pytest.mark.parametrize('allow_refresh', [True, False])
async def test_authorize_and_issue_token(app: Application, client: ClientSession, allow_refresh: bool):
    db_session = Session(bind=app[db.db_key])
    app_user = db_session.query(db.User).filter(db.User.username == 'username').one()

    conn = client.post('/auth', data={'username': 'username', 'password': 'password'})
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()

    app = db.App(
        user_id=app_user.id,
        client_id='x' * 24,
        redirect_uri='/auth',
        client_secret='x' * 48,
        token_endpoint_auth_method='client_secret_post',
        grant_type=(
            'authorization_code\r\nrefresh_token'
            if allow_refresh else
            'authorization_code'
        ),
        response_type='code',
        scope='smarthome another one',
        client_name='test',
        client_uri='http://localhost/',
    )
    db_session.add(app)
    db_session.commit()

    # request oauth authorization code
    conn = client.get(
        '/oauth/authorize',
        params={
            'client_id': app.client_id,
            'scope': 'smarthome',
            'response_type': 'code',
            'state': 'TEST',
        },
    )
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()

    # confirm oauth authorization code, should forward user to redirect_uri and provide code
    conn = client.post(
        '/oauth/authorize',
        params={
            'client_id': app.client_id,
            'scope': 'smarthome',
            'response_type': 'code',
            'state': 'TEST',
        },
        data={
            'confirm': 'true',
        },
        allow_redirects=False,
    )
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 302, await resp.text()
    url = yarl.URL(resp.headers['Location'])
    code = url.query['code']
    assert code

    # exchange code to token
    conn = client.post(
        '/oauth/token',
        data={
            'client_id': app.client_id,
            'client_secret': app.client_secret,
            'code': code,
            'grant_type': 'authorization_code',
        },
    )
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()
    data = await resp.json()
    assert data['access_token']
    assert data['token_type'] == 'Bearer'
    assert 'smarthome' in data['scope']
    assert allow_refresh is ('refresh_token' in data)

    token = db_session.query(db.Token).filter(db.Token.access_token == data['access_token']).one()
    assert token is not None

    if allow_refresh:
        token = db_session.query(db.Token).filter(db.Token.refresh_token == data['refresh_token']).one()
        assert token is not None


async def test_fetch_without_authorization(app: Application, client: TestClient):
    client.session.headers.add("Authorization", "Bearer garbage")

    conn = client.head('/v1.0/')
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()

    conn = client.get('/v1.0/user/devices')
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 401, await resp.text()

    for url in (
        '/v1.0/user/unlink',
        '/v1.0/user/devices/query',
        '/v1.0/user/devices/action',
    ):
        conn = client.post(url)
        resp = await asyncio.wait_for(conn, timeout=2.0)
        assert resp.status == 401, await resp.text()


async def test_fetch_with_authorization(app: Application, client: TestClient):
    db_session = Session(bind=app[db.db_key])

    app_user = db_session.query(db.User).filter(db.User.username == 'username').one()
    assert app_user is not None

    oauth_client = db.App(
        user_id=app_user.id,
        client_id='client',
        client_secret='secret',
        redirect_uri='/auth',
        token_endpoint_auth_method='client_secret_post',
        grant_type='authorization_code',
        response_type='code',
        scope='smarthome',
        client_name='test',
        client_uri='http://localhost/',
    )

    token = db.Token(
        user_id=app_user.id,
        client_id='client',
        token_type='Bearer',
        access_token='xxx',
        refresh_token='yyy',
        expires_in=600,
        scope='smarthome',
    )
    db_session.add(oauth_client)
    db_session.add(token)
    db_session.commit()

    client.session.headers.add('Authorization', 'Bearer xxx')

    conn = client.get('/v1.0/user/devices')
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()
    assert 'payload' in await resp.json()

    conn = client.post('/v1.0/user/devices/query', json={'devices': []})
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()
    assert 'payload' in await resp.json()

    conn = client.post('/v1.0/user/devices/action', json={'payload': {'devices': []}})
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()
    assert 'payload' in await resp.json()

    conn = client.post('/v1.0/user/unlink', data={
        'client_id': 'client',
        'client_secret': 'secret',
        'token': 'xxx',
    })
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()

    # now token is revoked and auth should fail
    conn = client.get('/v1.0/user/devices')
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 401, await resp.text()


async def test_profile_scopes(app: Application, client: TestClient):
    db_session = Session(bind=app[db.db_key])

    app_user = db_session.query(db.User).filter(db.User.username == 'username').one()
    assert app_user is not None

    token1 = db.Token(
        user_id=app_user.id,
        # NOTE (torkve) we happily skip creating client, because sqlite allows us.
        client_id='client',
        token_type='Bearer',
        access_token='xxx1',
        refresh_token='yyy1',
        expires_in=600,
        scope='smarthome',
    )
    token2 = db.Token(
        user_id=app_user.id,
        client_id='client',
        token_type='Bearer',
        access_token='xxx2',
        refresh_token='yyy2',
        expires_in=600,
        scope='smarthome profile',
    )
    db_session.add(token1)
    db_session.add(token2)
    db_session.commit()

    client.session.headers.add('Authorization', 'Bearer xxx')

    conn = client.get('/me', headers={'Authorization': 'Bearer xxx1'})
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 403, await resp.text()

    conn = client.get('/me', headers={'Authorization': 'Bearer xxx2'})
    resp = await asyncio.wait_for(conn, timeout=2.0)
    assert resp.status == 200, await resp.text()
    data = await resp.json()
    assert data['username'] == app_user.username
