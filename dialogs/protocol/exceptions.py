import typing

from .consts import QueryError, ActionError


class QueryException(Exception):
    def __init__(self, code: QueryError, message: typing.Optional[str] = None):
        super().__init__(message or code.value)
        self.code = code


class ActionException(Exception):
    def __init__(
        self,
        capability_id: str,
        instance: str,
        code: ActionError,
        message: typing.Optional[str] = None,
    ):
        super().__init__(message or code.value)
        self.capability_id = capability_id
        self.instance = instance
        self.code = code


class NotifyException(Exception):
    def __init__(
        self,
        request_id: typing.Optional[str],
        code: typing.Optional[str],
        message: typing.Optional[str],
    ):
        super().__init__(message or code)
        self.request_id = request_id
        self.code = code

    @classmethod
    def from_response(cls, data: dict) -> "NotifyException":
        return cls(
            request_id=data.get('request_id'),
            code=data.get('error_code'),
            message=data.get('error_message'),
        )
