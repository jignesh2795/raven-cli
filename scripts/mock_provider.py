#!/usr/bin/env python3
"""Tiny OpenAI-compatible SSE fixture for end-to-end tests.

First request: asks to read README.md.
Second request (after the tool result): streams a short final answer.

Usage:
    python scripts/mock_provider.py --port 8765

This is a test fixture, not a provider implementation. It has no auth and
should only listen on loopback.
"""

from __future__ import annotations

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args: object) -> None:  # keep test output clean
        pass

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        try:
            body = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            self.send_error(400, "invalid JSON")
            return

        messages = body.get("messages", [])
        has_tool_result = any(message.get("role") == "tool" for message in messages)

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True

        if not has_tool_result:
            self._data(
                {
                    "choices": [
                        {
                            "delta": {
                                "role": "assistant",
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "call_read_readme",
                                        "type": "function",
                                        "function": {
                                            "name": "read_file",
                                            "arguments": json.dumps({"path": "README.md"}),
                                        },
                                    }
                                ],
                            },
                            "finish_reason": None,
                        }
                    ]
                }
            )
            self._data({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]})
            self._usage(input_tokens=120, output_tokens=18)
        else:
            for text in [
                "The README describes a Windows-first agent CLI. ",
                "This mock confirms that the tool loop and event log work.\n",
            ]:
                self._data(
                    {
                        "choices": [
                            {"delta": {"content": text}, "finish_reason": None}
                        ]
                    }
                )
            self._data({"choices": [{"delta": {}, "finish_reason": "stop"}]})
            self._usage(input_tokens=180, output_tokens=24)

        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def _data(self, payload: object) -> None:
        self.wfile.write(b"data: ")
        self.wfile.write(json.dumps(payload).encode("utf-8"))
        self.wfile.write(b"\n\n")
        self.wfile.flush()

    def _usage(self, input_tokens: int, output_tokens: int) -> None:
        self._data(
            {
                "choices": [],
                "usage": {
                    "prompt_tokens": input_tokens,
                    "completion_tokens": output_tokens,
                    "prompt_tokens_details": {"cached_tokens": 0},
                },
            }
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()

    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"mock provider listening on http://127.0.0.1:{args.port}/v1", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
