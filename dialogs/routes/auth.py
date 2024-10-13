import json
import typing

import aiohttp_jinja2
from aiohttp import web
from aiohttp_security import (
    remember,
    forget,
    authorized_userid,
    check_authorized,
)

from dialogs import db, oauth


route = web.RouteTableDef()


@route.get('/auth', name='auth')
@aiohttp_jinja2.template('index.jinja2')
async def index(request: web.Request):
    user_id = await authorized_userid(request)

    user = None
    clients: typing.List[db.App] = []
    tokens: typing.List[db.Token] = []
    codes: typing.List[db.AuthorizationCode] = []
    if user_id:
        db_session = db.Session()
        user = db_session.get(db.User, int(user_id))
        clients = db_session.query(db.App).all()
        codes = db_session.query(db.AuthorizationCode).filter_by(user_id=user_id).all()
        tokens = db_session.query(db.Token).filter_by(user_id=user_id).all()

    return {
        'user': user,
        'clients': clients,
        'post_query': request.url.raw_query_string,
        'tokens': tokens,
        'codes': codes,
    }


@route.post('/auth', name='auth')
async def auth_post(request: web.Request) -> web.Response:
    form = await request.post()
    username = form.get('username')
    password = form.get('password')
    user = db.Session().query(db.User).filter_by(username=username, password=password).first()

    if user:
        response = web.HTTPFound(location=request.url)
        await remember(request, response, str(user.id))

        raise response

    raise web.HTTPForbidden(body=b'Who are you? Go away!')


@route.post('/auth/logout', name='logout')
async def auth_logout(request: web.Request) -> typing.NoReturn:
    response = web.HTTPFound(location=request.app.router['auth'].url_for().update_query(request.url.query))
    await forget(request, response)
    raise response


@route.get('/oauth/authorize', name='oauth')
@aiohttp_jinja2.template('oauth.jinja2')
async def oauthorize_get(request: web.Request):
    user_id = await authorized_userid(request)
    user = None
    if not user_id:
        raise web.HTTPFound(location=request.app.router['auth'].url_for())

    user = db.Session().get(db.User, int(user_id))
    assert user is not None

    try:
        grant = await request.app[oauth.server_key].get_consent_grant(request, user)
    except oauth.OAuth2Error as error:
        status, body, headers = error()
        exc = web.HTTPBadRequest(text=json.dumps(body), headers=headers)
        raise exc

    return {
        'user': user,
        'grant': grant,
    }


@route.post('/oauth/authorize', name='oauth')
async def oauthorize_post(request: web.Request):
    # login = await authorized_userid(request)
    # if login is None:
    #     url = URL(request.app.router["app"].url_for())
    #     raise web.HTTPFound(location=url.update_query({'redirect': request.rel_url}))

    user_id = await check_authorized(request)
    user = db.Session().get(db.User, int(user_id))

    form = await request.post()
    grant_user = user if form.get('confirm') else None
    return await request.app[oauth.server_key].create_authorization_response(request, grant_user)


@route.get('/oauth/token', name='token')
@route.post('/oauth/token', name='token')
async def token_post(request: web.Request) -> web.Response:
    return await request.app[oauth.server_key].create_token_response(request)


@route.post('/oauth/revoke', name='revoke')
async def revoke_post(request: web.Request) -> web.Response:
    return await request.app[oauth.server_key].create_endpoint_response('revocation', request)


@route.get('/me')
@oauth.resource_protected('profile')
async def me_get(request: web.Request) -> web.Response:
    token = request['oauth_token']
    user = token.user
    return web.json_response({'id': user.id, 'username': user.username})
