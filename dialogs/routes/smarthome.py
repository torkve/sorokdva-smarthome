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
    result = await request.app['oauth_server'].create_endpoint_response('revocation', request)
    return web.json_response({'request_id': request_id})


@route.get('/v1.0/user/devices', name='list_devices')
@resource_protected('smarthome')
async def list_devices(request: web.Request) -> web.Response:
    user = request['oauth_token'].user
    request_id = request.headers.get('X-Request-Id')
    return web.json_response({
        'request_id': request_id,
        'payload': {
            'user_id': user.username,
            'devices': [
                {
                    'id': 'freezer',
                    'name': 'Холодильник',
                    'type': 'devices.types.other',
                    'capabilities': [
                        {
                            'type': 'devices.capabilities.range',
                            'retrievable': True,
                            'parameters': {
                                'instance': 'temperature',
                                'unit': 'unit.temperature.celsius',
                                'range': {
                                    'min': -100.0,
                                    'max': 100.0,
                                },
                            },
                        },
                        {
                            'type': 'devices.capabilities.range',
                            'retrievable': True,
                            'parameters': {
                                'instance': 'humidity',
                                'unit': 'unit.percent',
                            },
                        },
                    ],
                },
            ],
        },
    })


@route.post('/v1.0/user/devices/query', name='query_devices')
@resource_protected('smarthome')
async def query_devices(request: web.Request) -> web.Response:
    user = request['oauth_token'].user
    request_id = request.headers.get('X-Request-Id')
    query = await request.json()
    response = {
        'request_id': request_id,
        'payload': {
            'devices': [
            ]
        },
    }
    for item in query['payload']['devices']:
        if item['id'] != 'freezer':
            response['payload']['devices'].append({
                'id': item['id'],
                'error_code': 'DEVICE_NOT_FOUND',
                'error_message': 'Устройство неизвестно',
            })
        else:
            response['payload']['devices'].append({
                'id': item['id'],
                'capabilities': [
                    {
                        'type': 'devices.capabilities.range',
                        'state': {
                            'instance': 'temperature',
                            'value': 42,
                        }
                    },
                    {
                        'type': 'devices.capabilities.range',
                        'state': {
                            'instance': 'humidity',
                            'value': 24,
                        }
                    },
                ],
            })
    return web.json_response(response)


@route.post('/v1.0/user/devices/action', name='control_devices')
@resource_protected('smarthome')
async def control_devices(request: web.Request) -> web.Response:
    user = request['oauth_token'].user
    request_id = request.headers.get('X-Request-Id')
    response = {
        'request_id': request_id,
        'payload': {
            'devices': [
            ]
        },
    }
    query = await request.json()
    for item in query['payload']['devices']:
        if item['id'] != 'freezer':
            response['payload']['devices'].append({
                'id': item['id'],
                'error_code': 'DEVICE_NOT_FOUND',
                'error_message': 'Устройство неизвестно',
            })
        else:
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
