import typing
import asyncio
import logging

from gmqtt import Client, constants


TopicName = str
Payload = str
ValueCallback = typing.Callable[[TopicName, Payload], typing.Awaitable[None]]


class MqttClient:
    def __init__(self, host: str, port: int, user: str, password: typing.Optional[str] = None):
        self.subscriptions: typing.Dict[str, typing.List[ValueCallback]] = {}
        self.host = host
        self.port = port
        self.client = Client('sorokdva-dialogs')
        self.client.set_auth_credentials(user, password)
        self.client.on_connect = self._on_connect
        self.client.on_message = self._on_message

    async def _on_message(
        self,
        client: Client,
        topic: str,
        payload: bytes,
        qos,
        properties,
    ) -> constants.PubRecReasonCode:
        log = logging.getLogger('mqtt')
        futures = []
        value = payload.decode()
        for cb in self.subscriptions.get(topic, []):
            log.info('passing (%r, %r) to %s', topic, value, cb)
            futures.append(cb(topic, value))

        if futures:
            await asyncio.wait(futures, return_when=asyncio.ALL_COMPLETED)

        return constants.PubRecReasonCode.SUCCESS

    def _on_connect(self, client: Client, flags: int, result: int, properties) -> None:
        # FIXME make base path configurable
        self.client.subscribe('/devices/#')

    def subscribe(self, topic: str, callback: ValueCallback) -> None:
        self.subscriptions.setdefault(topic, []).append(callback)

    def send(self, topic: str, message):
        self.client.publish(topic, message)

    async def run(self):
        await self.client.connect(self.host, self.port, version=constants.MQTTv311, keepalive=30)
        while True:
            self.client.publish('smarthome', b'ping')
            await asyncio.sleep(10)
