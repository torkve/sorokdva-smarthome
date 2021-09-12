import abc
import enum
import typing
import asyncio

if typing.TYPE_CHECKING:
    from mypy_extensions import KwArg

from .exceptions import ActionException, QueryException
from .consts import ActionError, ActionStatus


S = typing.TypeVar('S')
C = typing.TypeVar('C', bound='Capability')
if typing.TYPE_CHECKING:
    ChangeValue = typing.Optional[typing.Callable[
        [C, str, S, KwArg(typing.Any)],
        typing.Awaitable[typing.Tuple[str, str]]
    ]]
else:
    # in production we have no mypy_extensions and KwArg
    ChangeValue = typing.Optional[typing.Callable[
        [C, str, S],
        typing.Awaitable[typing.Tuple[str, str]]
    ]]


class Capability(typing.Generic[S], metaclass=abc.ABCMeta):
    def __init__(
        self: C,
        instances: typing.Iterable[str],
        initial_value: typing.Optional[S],
        change_value: ChangeValue[C, S] = None,
        retrievable: bool = False,
        reportable: bool = False,
    ):
        self._instances = list(instances)
        self._value = initial_value
        self._retrievable = retrievable
        self._reportable = reportable
        self.change_value = change_value or self._change_value_is_not_supported

    @staticmethod
    async def _change_value_is_not_supported(
        capability: C,
        instance: str,
        value: S,
        /,
        **kwargs
    ) -> typing.NoReturn:
        raise ActionException(capability.type_id, instance, ActionError.NotSupportedInCurrentMode)

    def handle_change(
        self: C,
        instance: str,
        value: S,
        kwargs: dict,
    ) -> typing.Awaitable[typing.Tuple[str, str]]:
        return self.change_value(self, instance, value, **kwargs)

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
    def reportable(self) -> bool:
        """
        If capability is reportable, it will push new state
        when its value is changed, not waiting for poll from server.
        """
        return self._reportable

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
    def value(self) -> typing.Optional[S]:
        if not self.retrievable:
            raise TypeError("Capability does not support retrieval")

        # FIXME should there be more common way to cast internal value to json representation?
        if isinstance(self._value, enum.Enum):
            return self._value.value

        return self._value

    @value.setter
    def value(self, value: S) -> None:
        self._value = value

        if self.reportable:
            self.report_new_value(value)

    def report_new_value(self, value: S) -> None:
        pass

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
            'reportable': self.reportable,
        }

        parameters = self.parameters
        if parameters is not None:
            response['parameters'] = parameters

        return response


class SingleInstanceCapability(Capability):
    def __init__(
        self: C,
        instance: str,
        initial_value: typing.Optional[S],
        change_value: ChangeValue[C, S] = None,
        retrievable: bool = False,
        reportable: bool = False,
    ):

        super().__init__(  # type: ignore
            instances=[instance],
            initial_value=initial_value,
            change_value=change_value,
            retrievable=retrievable,
            reportable=reportable,
        )

    @property
    def instance(self) -> str:
        return next(iter(self.instances))


class Property(typing.Generic[S], metaclass=abc.ABCMeta):
    def __init__(
        self,
        instance: str,
        initial_value: S,
        retrievable: bool = True,
        reportable: bool = False,
    ):
        if not retrievable and not reportable:
            raise TypeError("Non-retrievable property must be reportable")

        self.instance = instance
        self._value = initial_value
        self._retrievable = retrievable
        self._reportable = reportable

    @property
    @abc.abstractmethod
    def type_id(self) -> str:
        """
        Property type id.
        """

    @property
    def value(self) -> S:
        if not self.retrievable:
            raise TypeError("Property does not support retrieval")

        return self._value

    @value.setter
    def value(self, value: S) -> None:
        self._value = value

        if self.reportable:
            self.report_new_value(value)

    @property
    def retrievable(self) -> bool:
        """
        If property is retrievable, it can be queried by the server.
        Otherwise it must be reportable and send its state by its own.
        """
        return self._retrievable

    @property
    def reportable(self) -> bool:
        """
        If property is reportable, it will push new state
        when its value is changed, not waiting for poll from server.
        """
        return self._reportable

    def report_new_value(self, value: S) -> None:
        pass

    async def state(self) -> typing.AsyncIterator[dict]:
        """
        Property current state.
        If value is not ready, returns nothing.
        """

        if self.value is not None:
            yield {
                'type': self.type_id,
                'state': {
                    'instance': self.instance,
                    'value': self.value,
                }
            }

    @abc.abstractmethod
    async def specification(self) -> dict:
        """
        Get the property specification in format required by device list
        API call.
        """


class Device(abc.ABC):
    def __init__(
        self,
        device_id: str,
        capabilities: typing.Iterable[Capability],
        properties: typing.Optional[typing.Iterable[Property]] = None,
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
        self._properties = {
            (prop.type_id, prop.instance): prop
            for prop in properties or []
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

    def properties(self) -> typing.Iterable[Property]:
        """
        This method must return available device properties.
        Any device can have any properties.
        """
        return set(self._properties.values())

    async def specification(self) -> dict:
        """
        Get the device specification in format required by device list
        API call.
        """
        result: dict = {
            'id': self.device_id,
            'type': self.type_id,
            'capabilities': [],
            'properties': [],
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

        for prop in self.properties():
            result['properties'].append(await prop.specification())

        return result

    async def state(self) -> dict:
        """
        This method must return state for all capabilities and properties that
        are marked as retrievable.
        """
        result: dict = {
            'id': self.device_id,
            'capabilities': [],
            'properties': [],
        }

        caps = [cap.state() for cap in self.capabilities() if cap.retrievable]
        props = [prop.state() for prop in self.properties() if prop.retrievable]

        try:
            async for state in (states for cap in caps async for states in cap):
                result['capabilities'].append(state)

            async for state in (states for prop in props async for states in prop):
                result['properties'].append(state)
        except QueryException as e:
            del result['capabilities']
            del result['properties']
            result['error_code'] = e.code.value
            result['error_message'] = e.args[0]
            return result

        return result

    @staticmethod
    def split_value(state: dict) -> typing.Tuple[typing.Any, dict]:
        val = state.pop('value')
        state.pop('instance')
        return val, state

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
            (cap['type'], cap['state']['instance']): self.split_value(cap['state'])
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
            asyncio.create_task(self._capabilities[cap_key].handle_change(
                cap_key[1],
                cap_value,
                kwargs
            ))
            for cap_key, (cap_value, kwargs) in changes.items()
            if cap_key in self._capabilities
        ]

        if not caps:
            return result

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
