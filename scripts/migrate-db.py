#!/usr/bin/env python3
"""One-time migration of the legacy sqlite database to the JSON store the
Rust server uses.

    python3 scripts/migrate-db.py db.sqlite db.json

Only the Python standard library is required. The sqlite file is opened
read-only and left untouched.
"""

import base64
import json
import sqlite3
import sys


def rows(conn, table):
    cursor = conn.execute(f"SELECT * FROM {table}")
    names = [d[0] for d in cursor.description]
    return [dict(zip(names, row)) for row in cursor.fetchall()]


def optional(value):
    return value if value is not None else None


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src, dst = sys.argv[1], sys.argv[2]

    conn = sqlite3.connect(f"file:{src}?mode=ro", uri=True)

    settings = []
    for row in rows(conn, "server_settings"):
        value = row["value"]
        if isinstance(value, str):
            value = value.encode()
        settings.append((row["option"], base64.b64encode(value).decode()))

    users = [
        {
            "id": row["id"],
            "username": row["username"] or "",
            "password": row["password"] or "",
        }
        for row in rows(conn, "user")
    ]

    clients = [
        {
            "id": row["id"],
            "user_id": row["user_id"],
            "client_id": row["client_id"] or "",
            "client_secret": row["client_secret"] or "",
            "issued_at": row["issued_at"],
            "expires_at": row["expires_at"],
            "redirect_uri": row["redirect_uri"] or "",
            "token_endpoint_auth_method": row["token_endpoint_auth_method"] or "",
            "grant_type": row["grant_type"] or "",
            "response_type": row["response_type"] or "",
            "scope": row["scope"] or "",
            "client_name": row["client_name"] or "",
            "client_uri": row["client_uri"] or "",
        }
        for row in rows(conn, "app")
    ]

    codes = [
        {
            "id": row["id"],
            "user_id": row["user_id"],
            "client_id": row["client_id"] or "",
            "code": row["code"],
            "redirect_uri": row["redirect_uri"] or "",
            "scope": row["scope"] or "",
            "auth_time": row["auth_time"],
        }
        for row in rows(conn, "oauth2_code")
    ]

    tokens = [
        {
            "id": row["id"],
            "user_id": row["user_id"],
            "client_id": row["client_id"] or "",
            "access_token": row["access_token"],
            "refresh_token": optional(row["refresh_token"]),
            "scope": row["scope"] or "",
            "revoked": bool(row["revoked"]),
            "issued_at": row["issued_at"],
            "expires_in": row["expires_in"],
        }
        for row in rows(conn, "oauth2_token")
    ]

    data = {
        "version": 1,
        "settings": settings,
        "users": users,
        "clients": clients,
        "codes": codes,
        "tokens": tokens,
    }
    with open(dst, "x") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
        f.write("\n")

    print(
        f"migrated: {len(users)} users, {len(clients)} clients, "
        f"{len(tokens)} tokens, {len(codes)} codes, {len(settings)} settings"
    )


if __name__ == "__main__":
    main()
