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
