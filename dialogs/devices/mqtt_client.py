import typing
import asyncio
import logging

from gmqtt import Client, constants


class MqttClient:
    def __init__(self, host: str, port: int, user: str, password: typing.Optional[str] = None):
        self.subscriptions = {}
        self.host = host
        self.port = port
        self.client = Client('sorokdva-dialogs')
        self.client.set_auth_credentials(user, password)
        self.client.on_message = self.on_message

    async def on_message(self, client: Client, topic: str, payload: bytes, qos, properties) -> constants.PubRecReasonCode:
        log = logging.getLogger('mqtt')
        futures = []
        payload = payload.decode()
        for cb in self.subscriptions.get(topic, []):
            log.info('passing (%r, %r) to %s', topic, payload, cb)
            futures.append(cb(topic=topic, payload=payload))

        if futures:
            await asyncio.wait(futures, return_when=asyncio.ALL_COMPLETED)

        return constants.PubRecReasonCode.SUCCESS

    def subscribe(self, topic: str, callback) -> None:
        self.subscriptions.setdefault(topic, []).append(callback)

    def send(self, topic: str, message):
        self.client.publish(topic, message)

    async def run(self):
        await self.client.connect(self.host, self.port, version=constants.MQTTv311, keepalive=30)
        # FIXME make base path configurable
        self.client.subscribe('/devices/#')
        while True:
            self.client.publish('smarthome', b'ping')
            await asyncio.sleep(10)
