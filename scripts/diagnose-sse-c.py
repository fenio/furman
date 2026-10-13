#!/usr/bin/env python3
"""Read-only Boto3 comparison using Furman's saved macOS keychain entries.

Never enable SDK debug logging: HTTP headers contain encryption secrets.
"""

import argparse
import base64
import hashlib
import json
import logging
from pathlib import Path
import subprocess
import sys
from urllib.parse import urlsplit


def keychain_secret(account):
    result = subprocess.run(
        ["/usr/bin/security", "find-generic-password", "-s", "com.furman.s3", "-a", account, "-w"],
        capture_output=True,
        check=False,
    )
    if result.returncode:
        raise ValueError("Keychain access failed or was cancelled; no credential values were logged.")
    # security adds a newline; preserve any whitespace inside the stored secret.
    return result.stdout.removesuffix(b"\n").decode("utf-8")


def key_fields(encoded):
    raw = base64.b64decode(encoded, validate=True)
    if len(raw) != 32:
        raise ValueError("Stored SSE-C key is not a base64-encoded 32-byte key.")
    return raw, base64.b64encode(hashlib.md5(raw).digest()).decode("ascii")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", required=True, help="Saved Furman profile name or ID")
    parser.add_argument("--key", required=True, help="Exact object key, not an s3:// URL")
    parser.add_argument("--version-id", help="Optional exact historical version")
    args = parser.parse_args()
    if sys.platform != "darwin":
        raise ValueError("This diagnostic currently supports macOS keychain only.")

    import boto3
    from botocore.config import Config
    from botocore.exceptions import ClientError

    logging.disable(logging.CRITICAL)
    config = json.loads((Path.home() / ".config/furman/config.json").read_text())
    matches = [p for p in config.get("connections", [])
               if p.get("type", "s3") == "s3" and args.profile in (p.get("id"), p.get("name"))]
    if len(matches) != 1:
        raise ValueError("Select one unambiguous saved S3 profile name or ID.")
    profile = matches[0]
    if not profile.get("sseCustomerKeyEnabled"):
        raise ValueError("SSE-C is not enabled in this saved profile.")
    endpoint = profile.get("endpoint")
    if endpoint and urlsplit(endpoint).scheme != "https":
        raise ValueError("SSE-C diagnostic requires HTTPS.")
    if profile.get("roleArn") or profile.get("proxyUrl"):
        raise ValueError("Assumed roles/proxies are not supported by this diagnostic.")
    credential_type = profile.get("credentialType", "default")
    if credential_type not in ("keychain", "aws-profile", "default"):
        raise ValueError("This diagnostic requires keychain, AWS-profile, or default credentials.")

    secrets = []
    client_args = {"region_name": profile["region"], "endpoint_url": endpoint}
    if credential_type == "keychain":
        secret = keychain_secret(profile["id"])
        secrets.append(secret)
        client_args.update(aws_access_key_id=profile["accessKeyId"], aws_secret_access_key=secret)
    encoded = keychain_secret(profile["id"] + ":sse-c")
    raw, digest = key_fields(encoded)
    secrets.extend([encoded, digest])
    session = boto3.Session(profile_name=profile.get("profile") if credential_type == "aws-profile" else None)
    client = session.client("s3", **client_args, config=Config(
        signature_version="s3v4", retries={"total_max_attempts": 1},
        connect_timeout=10, read_timeout=10,
        s3={"addressing_style": "path" if endpoint else "auto"},
        proxies={},
    ))

    def verify_request(request, **_):
        # Independent check of actual Boto3 headers; no secret values printed.
        headers = {str(name).lower(): value for name, value in request.headers.items()}
        key = headers["x-amz-server-side-encryption-customer-key"]
        md5 = headers["x-amz-server-side-encryption-customer-key-md5"]
        if base64.b64decode(key, validate=True) != raw or base64.b64decode(md5, validate=True) != hashlib.md5(raw).digest():
            raise ValueError("Boto3 outgoing SSE-C header consistency check failed.")

    client.meta.events.register("before-send.s3", verify_request)
    common = {"Bucket": profile["bucket"], "Key": args.key, "SSECustomerAlgorithm": "AES256"}
    if args.version_id:
        common["VersionId"] = args.version_id
    for mode, fields in [
        ("automatic", {"SSECustomerKey": raw}),
        ("explicit", {"SSECustomerKey": encoded, "SSECustomerKeyMD5": digest}),
    ]:
        for operation in ("HEAD", "GET"):
            try:
                if operation == "HEAD":
                    response = client.head_object(**common, **fields)
                else:
                    response = client.get_object(**common, **fields, Range="bytes=0-0")
                    response["Body"].close()  # Never read, save, or print payload bytes.
                print(f"{mode} {operation}: HTTP {response['ResponseMetadata']['HTTPStatusCode']}; "
                      f"SSE-C confirmed={response.get('SSECustomerAlgorithm') == 'AES256'}")
            except ClientError as error:
                code = str(error.response.get("Error", {}).get("Code", "unknown"))
                message = str(error.response.get("Error", {}).get("Message", "No service message"))
                for secret in secrets:
                    if secret:
                        code = code.replace(secret, "[REDACTED]")
                        message = message.replace(secret, "[REDACTED]")
                status = error.response.get("ResponseMetadata", {}).get("HTTPStatusCode", "unknown")
                print(f"{mode} {operation}: HTTP {status}; {code}: {message}")
            except Exception:
                # SDK exception repr/debug output can contain sensitive headers.
                print(f"{mode} {operation}: transport/request failure (details withheld to protect secrets)")
    client.close()


if __name__ == "__main__":
    try:
        main()
    except (ValueError, ImportError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    except Exception:
        print("Diagnostic setup failed; details withheld to protect secrets.", file=sys.stderr)
        sys.exit(1)
