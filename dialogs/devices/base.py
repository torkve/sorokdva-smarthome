import abc
import typing
import asyncio

from .exceptions import ActionException, QueryException
from .consts import ActionError, ActionStatus


S = typing.TypeVar('S')
ChangeValue = typing.Optional[typing.Callable[
    [S],
    typing.Awaitable[typing.Tuple[str, str]]
]]


class Capability(abc.ABC):
    def __init__(
        self,
        instances: typing.Iterable[str],
        initial_value: typing.Any,
        change_value: ChangeValue[typing.Any] = None,
        retrievable: bool = False,
    ):
        self._instances = list(instances)
        self._value = initial_value
        self._retrievable = retrievable
        self.change_value = change_value or self._change_value_is_not_supported

    async def _change_value_is_not_supported(self, value: typing.Any) -> typing.NoReturn:
        raise ActionException(self.type_id, self.instance, ActionError.NotSupportedInCurrentMode)

    @property
    @abc.abstractmethod
    def type_id(self) -> str:
        """
        Capability type id.
        """

    @property
    def retrievable(self) -> bool:
        """
        If capability is retrievable, it can be queried,
        not only set.
        """
        return self._retrievable

    @property
    def instances(self) -> typing.Iterable[str]:
        """
        Capability instance name
        """
        return iter(self._instances)

    @property
    @abc.abstractmethod
    def parameters(self) -> typing.Optional[dict]:
        """
        If present, capability has additional specific parameters.
        """

    @property
    def value(self) -> typing.Any:
        if not self.retrievable:
            raise TypeError("Property does not support retrieval")

        return self._value

    @value.setter
    def value(self, value: typing.Any) -> None:
        self._value = value

    async def state(self) -> typing.AsyncIterator[dict]:
        """
        Capability current state, availble only for retrievable capabilities.
        If value is not ready, returns nothing.
        """

        if self.value is not None:
            yield {
                'type': self.type_id,
                'state': {
                    'instance': next(iter(self.instances)),
                    'value': self.value,
                }
            }

    async def specification(self) -> dict:
        """
        Get the capability specification in format required by device list
        API call.
        """
        response = {
            'type': self.type_id,
            'retrievable': self.retrievable,
        }

        parameters = self.parameters
        if parameters is not None:
            response['parameters'] = parameters

        return response


class SingleInstanceCapability(Capability):
    def __init__(
        self,
        instance: str,
        initial_value: typing.Any,
        change_value: ChangeValue[typing.Any] = None,
        retrievable: bool = False,
    ):
        super().__init__(
            instances=[instance],
            initial_value=initial_value,
            change_value=change_value,
            retrievable=retrievable,
        )

    @property
    def instance(self) -> str:
        return next(iter(self.instances))


class Device(abc.ABC):
    def __init__(
        self,
        device_id: str,
        capabilities: typing.Iterable[Capability],
        device_name: typing.Optional[str] = None,
        description: typing.Optional[str] = None,
        room: typing.Optional[str] = None,
        custom_data: typing.Optional[dict] = None,
        manufacturer: typing.Optional[str] = None,
        model: typing.Optional[str] = None,
        hw_version: typing.Optional[str] = None,
        sw_version: typing.Optional[str] = None,
    ):
        self.device_id = device_id

        self.name = device_name
        self.description = description
        self.room = room
        self.custom_data = custom_data
        self.manufacturer = manufacturer
        self.model = model
        self.hw_version = hw_version
        self.sw_version = sw_version

        self._capabilities = {
            (cap.type_id, instance): cap
            for cap in capabilities
            for instance in cap.instances
        }

    @property
    @abc.abstractmethod
    def type_id(self) -> str:
        """Device type id"""

    async def updater_loop(self) -> None:
        """
        Task performing status update in a loop.
        Task should implement its own loop with desired
        period or just do nothing.
        """

    def capabilities(self) -> typing.Iterable[Capability]:
        """
        This method must return available device capabilities.
        Most device types have some recommended set of ones, but
        any device can have any capabilities.
        """
        return set(self._capabilities.values())

    async def specification(self) -> dict:
        """
        Get the device specification in format required by device list
        API call.
        """
        result: dict = {
            'id': self.device_id,
            'type': self.type_id,
            'capabilities': [],
        }
        for field in ('name', 'description', 'room', 'custom_data'):
            value = getattr(self, field)
            if value is not None:
                result[field] = value

        for field in ('manufacturer', 'model', 'hw_version', 'sw_version'):
            value = getattr(self, field)
            if value is not None:
                result.setdefault('device_info', {})[field] = value

        for cap in self.capabilities():
            result['capabilities'].append(await cap.specification())

        return result

    async def state(self) -> dict:
        """
        This method must return state for all capabilities, that
        are marked as retrievable.
        """
        result: dict = {
            'id': self.device_id,
            'capabilities': [],
        }

        caps = [cap.state() for cap in self.capabilities() if cap.retrievable]

        try:
            async for state in (states for cap in caps async for states in cap):
                result['capabilities'].append(state)
        except QueryException as e:
            del result['capabilities']
            result['error_code'] = e.code.value
            result['error_message'] = e.args[0]
            return result

        return result

    async def action(
        self,
        capabilities: typing.Iterable[dict],
        custom_data: typing.Optional[dict],
    ) -> dict:
        result: dict = {
            'id': self.device_id,
            'capabilities': []
        }

        changes = {
            (cap['type'], cap['state']['instance']): cap['state']['value']
            for cap in capabilities
        }
        for cap_key in changes:
            if cap_key not in self._capabilities:
                # TODO make it better?
                result['capabilities'].append({
                    'type': cap_key[0],
                    'state': {
                        'instance': cap_key[1],
                        'action_result': {
                            'status': ActionStatus.Error.value,
                            'error_code': ActionError.InvalidAction.value,
                            'error_message': 'Unknown capability for this device',
                        }
                    }
                })

        caps = [
            self._capabilities[cap_key].change_value(cap_value)
            for cap_key, cap_value in changes.items()
            if cap_key in self._capabilities
        ]

        changes_ready, _ = await asyncio.wait(caps, return_when=asyncio.ALL_COMPLETED)

        for change in changes_ready:
            try:
                type_id, instance = await change
            except ActionException as e:
                result['capabilities'].append({
                    'type': e.capability_id,
                    'state': {
                        'instance': e.instance,
                        'action_result': {
                            'status': ActionStatus.Error.value,
                            'error_code': e.code.value,
                            'error_message': e.args[0],
                        }
                    },
                })
            else:
                result['capabilities'].append({
                    'type': type_id,
                    'state': {
                        'instance': instance,
                        'action_result': {
                            'status': ActionStatus.Done.value,
                        }
                    }
                })

        return result
