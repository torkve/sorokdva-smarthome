import os
import asyncio
import logging
import argparse

import aiohttp_session
import aiohttp_jinja2
import aiohttp_remotes
from aiohttp import web
from aiohttp_session.cookie_storage import EncryptedCookieStorage

from dialogs import db, oauth, auth
from dialogs.routes.auth import route as auth_route
from dialogs.routes.debug import route as debug_route
from dialogs.routes.smarthome import route as smarthome_route

from dialogs.devices.freezer import Freezer


async def make_app(args):
    app = web.Application()

    app.add_routes(auth_route)
    app.add_routes(smarthome_route)
    if args.debug:
        app.add_routes(debug_route)

    if args.proxy:
        await aiohttp_remotes.setup(
            app,
            aiohttp_remotes.XForwardedRelaxed(),
        )
    db.setup(app, f'sqlite:///{args.db}')
    oauth.setup(app)
    aiohttp_jinja2.setup(app, loader=aiohttp_jinja2.jinja2.FileSystemLoader('static'))
    aiohttp_jinja2.get_env(app).globals.update(
        url_for=lambda path: app.router[path].url_for(),
        DEBUG=args.debug,
    )

    db_session = db.session_maker()
    cookie_key = db_session.query(db.ServerSettings).filter_by(option='cookie_key').first()
    if not cookie_key:
        cookie_key = db.ServerSettings()
        cookie_key.option = 'cookie_key'
        cookie_key.value = open('/dev/urandom', 'rb').read(32)
        db_session.add(cookie_key)
        db_session.commit()

    aiohttp_session.setup(app, EncryptedCookieStorage(cookie_key.value))
    auth.setup(app)

    # FIXME make it better
    app['smarthome_devices'] = {
        'freezer': Freezer('freezer', 'Холодильник')
    }
    for device in app['smarhome_devices'].values():
        app.on_startup.append(device.updater)

    main_app = web.Application()
    main_app.add_subapp(args.prefix, app)
    return main_app


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        '-i', '--interface',
        default='127.0.0.1',
        help='Interface to listen on',
    )
    parser.add_argument(
        '-p', '--port',
        default=8080,
        type=int,
        help='Port to listen on',
    )
    parser.add_argument(
        '--prefix',
        default='/',
        help='Bind app to subpath',
    )
    parser.add_argument(
        '--db',
        default=':memory:',
        help='Path to database',
        metavar='FILENAME',
    )
    parser.add_argument(
        '--debug',
        action='store_true',
        default=bool(os.getenv('DIALOGS_DEBUG')),
        help='Enable debugging features',
    )
    parser.add_argument(
        '--proxy',
        action='store_true',
        default=not bool(os.getenv('AUTHLIB_INSECURE_TRANSPORT')),
        help='Enable running behind proxy (parse X-Forwarded-... headers)',
    )
    return parser.parse_args()


if __name__ == '__main__':
    args = parse_args()

    logging.basicConfig(
        level=logging.DEBUG if args.debug else logging.INFO,
        format='%(asctime)s.%(msecs)003d [%(levelname)-5s] [%(name)-10s]   %(message)s',
        datefmt='%Y-%m-%d %H:%M:%S'
    )
    logging.getLogger('sqlalchemy').setLevel(logging.INFO)

    app = asyncio.get_event_loop().run_until_complete(make_app(args))
    web.run_app(
        app,
        host=args.interface,
        port=args.port,
        access_log_format='%a "%r" %s %b "%{Referer}i" "%{User-Agent}i" "%{X-Request-Id}i"',
    )
