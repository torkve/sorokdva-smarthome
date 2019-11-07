import typing

from authlib.common.security import generate_token

import aiohttp_jinja2
import aiohttp_security
from aiohttp import web

from dialogs import db


route = web.RouteTableDef()


@route.post('/auth/register', name='auth_register')
async def register_user_post(request: web.Request) -> typing.NoReturn:
    form = await request.post()
    username = form['username']
    password = form['password']

    db_session = db.Session()
    user = db_session.query(db.User).filter_by(username=username).first()
    if user:
        raise web.HTTPBadRequest(text="User already exists")

    user = db.User(username=username, password=password)
    db_session.add(user)
    db_session.commit()

    response = web.HTTPFound(location=request.app.router['auth'].url_for())
    await aiohttp_security.remember(request, response, str(user.id))

    raise response


@route.get('/oauth/create-client', name='oauth_create_client')
@aiohttp_jinja2.template('create_client.jinja2')
async def create_client_get(request: web.Request):
    user_id = await aiohttp_security.authorized_userid(request)
    if user_id is None:
        raise web.HTTPFound(location=request.app.router['auth'].url_for())


@route.post('/oauth/create-client', name='oauth_create_client')
async def create_client_post(request: web.Request):
    user_id = await aiohttp_security.check_authorized(request)

    form = await request.post()
    app = db.App(**form)
    app.user_id = user_id
    app.client_id = generate_token(24)  # type: ignore
    app.client_secret = generate_token(48)  # type: ignore
    app.token_endpoint_auth_method = 'client_secret_post'  # type: ignore

    db_session = db.Session()
    db_session.add(app)
    db_session.commit()

    raise web.HTTPFound(location=request.app.router['auth'].url_for())
