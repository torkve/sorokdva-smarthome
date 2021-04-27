import time
import typing
import logging

import yarl
import aiohttp

from .base import Device
from .exceptions import NotifyException


class Notifications:
    def __init__(
        self,
        skill_id: str,
        user_id: str,
        oauth_token: str,
        log: typing.Optional[logging.Logger] = None,
    ):
        self.skill_id = skill_id
        self.user_id = user_id
        self.base_url = yarl.URL(
            'https://dialogs.yandex.net/api/v1/skills/'
        ).join(
            yarl.URL(self.skill_id)
        ).join(
            yarl.URL('callback')
        )
        self.log = log or logging.getLogger(__name__)
        self.session = aiohttp.ClientSession(
            headers={'Authorization': f'OAuth {oauth_token}'},
            timeout=aiohttp.ClientTimeout(total=30., connect=2.),
        )

    async def close(self):
        await self.session.close()

    async def send_device_state(self, *devices: Device):
        """
        Notify Yandex.Alice that devices states changed
        """
        url = self.base_url.join(yarl.URL('state'))
        ts = time.time()
        payload: dict = {
            'ts': ts,
            'payload': {
                'user_id': self.user_id,
                'devices': [],
            },
        }
        for device in devices:
            # FIXME get in parallel
            payload['payload']['devices'].append(await device.state())
        response = await self.session.post(url, json=payload)
        status = response.status
        data = await response.json()
        if 200 <= status < 300:
            self.log.info("Sent state, request_id=%r", data.get('request_id'))
            return

        self.log.error("Send state failed: %r", data)
        raise NotifyException.from_response(data)

    async def send_device_specifications_updated(self):
        url = self.base_url.join(yarl.URL('discovery'))
        ts = time.time()
        response = await self.session.post(url, json={
            'ts': ts,
            'payload': {
                'user_id': self.user_id,
            }
        })
        status = response.status
        data = await response.json()
        if 200 <= status < 300:
            self.log.info("Sent state, request_id=%r", data.get('request_id'))
            return

        self.log.error("Send device specification failed: %r", data)
        raise NotifyException.from_response(data)
