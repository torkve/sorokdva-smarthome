import time
import typing
import asyncio
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
            yarl.URL(f'{skill_id}/')
        ).join(
            yarl.URL('callback/')
        )
        self.log = log or logging.getLogger(__name__)
        self.session = aiohttp.ClientSession(
            headers={'Authorization': f'OAuth {oauth_token}'},
            timeout=aiohttp.ClientTimeout(total=30., connect=2.),
        )

    async def close(self):
        await self.session.close()

    async def send_device_states(self, devices: list[dict]):
        """
        Notify Yandex.Alice that devices states changed
        """
        url = self.base_url.join(yarl.URL('state'))
        ts = time.time()
        payload: dict = {
            'ts': ts,
            'payload': {
                'user_id': self.user_id,
                'devices': devices,
            },
        }
        response = await self.session.post(url, json=payload)
        status = response.status
        data = await response.json()
        if 200 <= status < 300:
            self.log.info("Sent state, request_id=%r", data.get('request_id'))
            return

        self.log.error("Send state failed: %r, url: %r", data, url)
        raise NotifyException.from_response(data)

    async def notifications_loop(self, devices: dict[str, Device], initial_state: dict) -> None:
        previous_state = initial_state
        while True:
            # 10 seconds value is hardcoded as a sane period that should not lead to ban
            # from Alice server.
            await asyncio.sleep(10)

            states = []
            # FIXME get in parallel
            for device_id, device in devices.items():
                try:
                    device_state = await device.report(previous_state.get(device_id, {}))
                    changed_capabilities = [val for val, changed in device_state['capabilities'] if changed]
                    changed_properties = [val for val, changed in device_state['properties'] if changed]
                    if changed_capabilities or changed_properties:
                        states.append({
                            'id': device_state['id'],
                            'capabilities': changed_capabilities,
                            'properties': changed_properties,
                        })
                    previous_state['device_id'] = {
                        'id': device_state['id'],
                        'capabilities': [val for val, changed in device_state['capabilities']],
                        'properties': [val for val, changed in device_state['properties']],
                    }
                except Exception:
                    self.log.exception("Failed to query device %r report", device_id)

            if states:
                try:
                    await self.send_device_states(states)
                except Exception:
                    pass

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

        self.log.error("Send device specification failed: %r, url: %r", data, url)
        raise NotifyException.from_response(data)
