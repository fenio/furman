"""No credentials, keychain access, or network needed for these tests."""
import base64
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("diagnostic", Path(__file__).parents[1] / "scripts/diagnose-sse-c.py")
diagnostic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(diagnostic)


class DiagnosticTests(unittest.TestCase):
    def test_independent_key_vector_and_invalid_lengths(self):
        key = b"0123456789abcdef0123456789abcdef"
        self.assertEqual(diagnostic.key_fields(base64.b64encode(key)), (key, "hRasmdxgYDKV3nvbahU1MA=="))
        with self.assertRaises(ValueError):
            diagnostic.key_fields(base64.b64encode(b"short"))

    def test_only_reads_and_never_consumes_or_prints_payload(self):
        key = b"0123456789abcdef0123456789abcdef"
        encoded = base64.b64encode(key).decode()
        body = Mock()
        client = Mock()
        client.head_object.return_value = {"ResponseMetadata": {"HTTPStatusCode": 200}, "SSECustomerAlgorithm": "AES256"}
        client.get_object.return_value = {"ResponseMetadata": {"HTTPStatusCode": 206}, "SSECustomerAlgorithm": "AES256", "Body": body}
        boto3 = types.ModuleType("boto3")
        boto3.Session = Mock(return_value=Mock(client=Mock(return_value=client)))
        config_module = types.ModuleType("botocore.config")
        config_module.Config = Mock()
        exceptions = types.ModuleType("botocore.exceptions")
        exceptions.ClientError = type("ClientError", (Exception,), {})
        modules = {"boto3": boto3, "botocore": types.ModuleType("botocore"),
                   "botocore.config": config_module, "botocore.exceptions": exceptions}
        profile = {"type": "s3", "name": "test", "id": "test-id", "bucket": "test-bucket",
                   "region": "us-east-1", "endpoint": "https://example.invalid",
                   "accessKeyId": "test-access", "credentialType": "keychain", "sseCustomerKeyEnabled": True}
        output = io.StringIO()
        with tempfile.TemporaryDirectory() as home:
            config_dir = Path(home) / ".config/furman"
            config_dir.mkdir(parents=True)
            (config_dir / "config.json").write_text(json.dumps({"connections": [profile]}))
            with patch.object(diagnostic.Path, "home", return_value=Path(home)), \
                 patch.object(diagnostic, "keychain_secret", side_effect=["test-secret", encoded]), \
                 patch.object(sys, "argv", ["diagnostic", "--profile", "test", "--key", "object", "--version-id", "version"]), \
                 patch.object(sys, "platform", "darwin"), patch.dict(sys.modules, modules), \
                 patch.object(diagnostic.logging, "disable"), contextlib.redirect_stdout(output):
                diagnostic.main()
        self.assertEqual(client.head_object.call_count, 2)
        self.assertEqual(client.get_object.call_count, 2)
        self.assertEqual(body.close.call_count, 2)
        body.read.assert_not_called()
        client.put_object.assert_not_called()
        client.delete_object.assert_not_called()
        for call in client.get_object.call_args_list:
            self.assertEqual(call.kwargs["Range"], "bytes=0-0")
            self.assertEqual(call.kwargs["VersionId"], "version")
        self.assertEqual(client.head_object.call_args_list[0].kwargs["SSECustomerKey"], key)
        self.assertEqual(client.head_object.call_args_list[1].kwargs["SSECustomerKey"], encoded)
        self.assertNotIn(encoded, output.getvalue())
        self.assertNotIn("test-secret", output.getvalue())
        # Exercise the actual outgoing-header validation callback too.
        verifier = client.meta.events.register.call_args.args[1]
        request = Mock(headers={"x-amz-server-side-encryption-customer-key": encoded,
                                "x-amz-server-side-encryption-customer-key-md5": "hRasmdxgYDKV3nvbahU1MA=="})
        verifier(request)
        request.headers["x-amz-server-side-encryption-customer-key-md5"] = base64.b64encode(bytes(16))
        with self.assertRaises(ValueError):
            verifier(request)


if __name__ == "__main__":
    unittest.main()
