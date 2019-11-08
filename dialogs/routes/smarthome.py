from aiohttp import web

from dialogs.oauth import resource_protected


route = web.RouteTableDef()


@route.head('/v1.0/', name='ping')
async def ping(request: web.Request):
    raise web.HTTPOk()


@route.post('/v1.0/user/unlink', name='unlink')
@resource_protected('smarthome')
async def user_unlink(request: web.Request) -> web.Response:
    request_id = request.headers.get('X-Request-Id')
    await request.app['oauth_server'].create_endpoint_response('revocation', request)
    return web.json_response({'request_id': request_id})


@route.get('/v1.0/user/devices', name='list_devices')
@resource_protected('smarthome')
async def list_devices(request: web.Request) -> web.Response:
    user = request['oauth_token'].user
    request_id = request.headers.get('X-Request-Id')
    devices = request.app['smarthome_devices']
    return web.json_response({
        'request_id': request_id,
        'payload': {
            'user_id': user.username,
            'devices': [
                await device.features()
                for device in devices.values()
            ],
        },
    })


@route.post('/v1.0/user/devices/query', name='query_devices')
@resource_protected('smarthome')
async def query_devices(request: web.Request) -> web.Response:
    request_id = request.headers.get('X-Request-Id')
    query = await request.json()
    response = {
        'request_id': request_id,
        'payload': {
            'devices': [
            ]
        },
    }
    devices = request.app['smarthome_devices']

    for item in query['devices']:
        if item['id'] not in devices:
            response['payload']['devices'].append({
                'id': item['id'],
                'error_code': 'DEVICE_NOT_FOUND',
                'error_message': 'Устройство неизвестно',
            })
        else:
            response['payload']['devices'].append(await devices[item['id']].query())
    return web.json_response(response)


@route.post('/v1.0/user/devices/action', name='control_devices')
@resource_protected('smarthome')
async def control_devices(request: web.Request) -> web.Response:
    request_id = request.headers.get('X-Request-Id')
    response = {
        'request_id': request_id,
        'payload': {
            'devices': [
            ]
        },
    }
    devices = request.app['smarthome_devices']
    query = await request.json()
    for item in query['payload']['devices']:
        if item['id'] not in devices:
            response['payload']['devices'].append({
                'id': item['id'],
                'error_code': 'DEVICE_NOT_FOUND',
                'error_message': 'Устройство неизвестно',
            })
        else:
            # FIXME get from device
            item_result = {
                'id': item['id'],
                'action_result': {
                    'status': 'ERROR',
                    'error_code': 'INVALID_ACTION',
                    'error_message': 'Устройство поддерживает только запрос состояния, но не управление.',
                },
            }
            response['payload']['devices'].append(item_result)

    return web.json_response(response)
