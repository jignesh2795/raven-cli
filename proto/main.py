"""Raven CLI M0 prototype: read-only tool loop (Python)."""
import os, json, sys
try:
    from dotenv import load_dotenv
    load_dotenv()
except ImportError:
    pass
from openai import OpenAI

BASE_URL = os.getenv("RAVEN_BASE_URL", "https://integrate.api.nvidia.com/v1")
API_KEY = os.getenv("RAVEN_API_KEY", os.getenv("NVIDIA_API_KEY", ""))
MODEL = os.getenv("RAVEN_MODEL", "meta/llama-3.3-70b-instruct")

client = OpenAI(base_url=BASE_URL, api_key=API_KEY)

TOOLS = [
    {"type": "function", "function": {
        "name": "read_file",
        "description": "Read a file (read-only).",
        "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]},
    }},
    {"type": "function", "function": {
        "name": "list_dir",
        "description": "List directory contents.",
        "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]},
    }},
]

def read_file(path):
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as f:
            return f.read()[:20000]
    except Exception as e:
        return f"ERROR: {e}"

def list_dir(path):
    try:
        return "\n".join(sorted(os.listdir(path)))[:20000]
    except Exception as e:
        return f"ERROR: {e}"

DISPATCH = {"read_file": read_file, "list_dir": list_dir}

def run(prompt):
    messages = [{"role": "user", "content": prompt}]
    for _ in range(20):
        resp = client.chat.completions.create(model=MODEL, messages=messages, tools=TOOLS, tool_choice="auto")
        msg = resp.choices[0].message
        messages.append(msg)
        if not msg.tool_calls:
            print(msg.content)
            return
        for tc in msg.tool_calls:
            name = tc.function.name
            args = json.loads(tc.function.arguments or "{}")
            print(f"[tool] {name} {args}", file=sys.stderr)
            result = DISPATCH.get(name, lambda **a: "unknown tool")(**args)
            messages.append({"role": "tool", "tool_call_id": tc.id, "content": result})

if __name__ == "__main__":
    run(" ".join(sys.argv[1:]) or "List the current directory.")
