# Custom Codec

[RedisME](https://www.hepengju.com) supports custom serialization/deserialization via external scripts, so you can view and edit non–UTF-8 or application-specific payloads.

> **Built-in JavaSerial**: For STRING values, choose **JavaSerial** in the codec dropdown to view JDK-serialized bytes as plain text (top-level `String`) or JSON (other objects). Same approach as RedisInsight (`java-object-serialization`), with extras for `java.time`, records, common collections, and BitSet / StringBuilder / InetAddress / Calendar / sql.Date. **View-only**. To write back, use the custom scripts below with a local `java`.

## Entry and setup

1. Open the value detail view
2. Click the **edit** icon next to the **codec** dropdown to open the **Custom Codec** dialog
3. Add an entry:
   - **Name** — shown in the dropdown
   - **Command** — full executable command including the interpreter (see below)

![](../../../public/images/codec/main.png)

## Script protocol

The app appends **two arguments** after your command:

```bash
# Read: Redis raw bytes (wire base64) → editor text
{command} decode {wire_base64}

# Write: editor text → Redis raw bytes (wire base64)
{command} encode {editor_text_utf8_base64}

# When Base64 exceeds 8000 chars (avoids Windows command-line limits)
{command} decode --stdin    # read one line of Base64 from stdin
{command} encode --stdin
```

| Direction         | Arg 1    | Arg 2                                     | stdout                                    |
| ----------------- | -------- | ----------------------------------------- | ----------------------------------------- |
| **decode**        | `decode` | Base64 of raw Redis bytes                 | UTF-8 text (shown in the editor)          |
| **encode**        | `encode` | Base64 of editor text as UTF-8 bytes      | **Single line** Base64 of raw Redis bytes |
| **large payload** | same     | `--stdin` (one line of Base64 from stdin) | same                                      |

Notes:

- Arg 2 is a single-line standard Base64 string (no spaces); **over 8000 chars** it becomes `--stdin` — read one line from stdin (`readline` or equivalent)
- Small payloads still use `sys.argv[2]` (Python), `args[1]` (Java), or `process.argv[3]` (Node)
- On **decode** success, stdout is editable text; on **encode** success, stdout is a single Base64 line
- On failure, write to stderr and use a non-zero exit code; the app shows the full executed command in the error message
- Base64 in arg 2 is the **wire format** between the app and your script — not necessarily what you show in the editor (see the Hex sample below)

## Command examples

```
python C:\path\to\codec.py
node /path/to/codec.js
java C:\path\to\codec.java
```

::: tip stdout encoding
The app reads stdout as **UTF-8**. On Windows, Python scripts should call `sys.stdout.reconfigure(encoding='utf-8')` (see samples below).
:::

## Workflow

1. Configure and save your custom codec entry
2. Select it from the **codec** dropdown
3. The value area shows decoded text; edit and click **Save**
4. Use **Test Decode / Test Encode** in the dialog to verify your script:
   - Default wire Base64 sample is `aGVsbG8=` (bytes `hello`)
   - With the Hex samples below: **Test Decode** should show `68656c6c6f`; for **Test Encode**, use editor sample `68656c6c6f` (UTF-8 text) and expect `aGVsbG8=`

## Scope and limits

- Default execution timeout is 5 seconds
- Base64 ≤ 8000 chars uses the command line; above that the app uses `--stdin` automatically (scripts must support it; see samples)

## Sample: Python (Hex view/edit for binary)

Raw Redis bytes are shown as **lowercase hex** in the editor; on save, hex is parsed back to bytes.

```python
import sys
import base64
import binascii

# Windows pipe output may default to GBK; RedisME reads stdout as UTF-8
if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')

mode = sys.argv[1]
if len(sys.argv) > 2 and sys.argv[2] == '--stdin':
    b64 = sys.stdin.readline().strip()
else:
    b64 = sys.argv[2]
try:
    if mode == 'decode':
        print(binascii.hexlify(base64.b64decode(b64)).decode('ascii'))
    elif mode == 'encode':
        hex_str = base64.b64decode(b64).decode('utf-8').strip()
        print(base64.b64encode(binascii.unhexlify(hex_str)).decode('ascii'))
    else:
        raise ValueError(f'unknown mode: {mode}')
except (binascii.Error, ValueError) as e:
    print(str(e), file=sys.stderr)
    sys.exit(1)
```

Command example:

```
C:\path\to\python.exe C:\path\to\codec.py
```

## Sample: Node.js (Hex view/edit for binary)

```javascript
#!/usr/bin/env node
/** wire base64 ↔ hex text (same protocol as codec.py) */
const mode = process.argv[2]
const arg = process.argv[3]

function run(b64) {
  try {
    if (mode === 'decode') {
      process.stdout.write(Buffer.from(b64, 'base64').toString('hex'))
    } else if (mode === 'encode') {
      const hex = Buffer.from(b64, 'base64').toString('utf8').trim()
      process.stdout.write(Buffer.from(hex, 'hex').toString('base64'))
    } else {
      throw new Error(`unknown mode: ${mode}`)
    }
    process.exit(0) // app does not close stdin after write; exit explicitly
  } catch (e) {
    process.stderr.write(String(e) + '\n')
    process.exit(1)
  }
}

function readB64FromStdin() {
  return new Promise((resolve, reject) => {
    let buf = ''
    process.stdin.setEncoding('utf8')
    process.stdin.on('data', chunk => {
      buf += chunk
      const nl = buf.search(/\r?\n/)
      if (nl >= 0) {
        process.stdin.pause()
        resolve(buf.slice(0, nl).trim())
      }
    })
    process.stdin.on('error', reject)
    process.stdin.resume()
  })
}

if (arg === '--stdin') {
  readB64FromStdin()
    .then(run)
    .catch(e => {
      process.stderr.write(String(e) + '\n')
      process.exit(1)
    })
} else if (arg) {
  run(arg)
} else {
  process.stderr.write('usage: codec.js <decode|encode> <base64|--stdin>\n')
  process.exit(1)
}
```

Command example:

```
node C:\path\to\codec.js
```

## Sample: Java (Hex view/edit for binary)

Requires **JDK 11+**; you can run the single source file directly (no `javac` step). `args[0]` is the mode; `args[1]` is Base64 or `--stdin`.

```java
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.Scanner;

/** wire base64 ↔ hex text (same protocol as codec.py) */
public class codec {
    public static void main(String[] args) {
        if (args.length < 2) {
            System.err.println("usage: codec <decode|encode> <base64|--stdin>");
            System.exit(1);
        }
        String mode = args[0];
        String b64 = "--stdin".equals(args[1])
            ? new Scanner(System.in).nextLine().trim()
            : args[1];
        try {
            if ("decode".equals(mode)) {
                System.out.print(toHex(Base64.getDecoder().decode(b64)));
            } else if ("encode".equals(mode)) {
                String hex = new String(Base64.getDecoder().decode(b64), StandardCharsets.UTF_8).trim();
                System.out.print(Base64.getEncoder().encodeToString(fromHex(hex)));
            } else {
                throw new IllegalArgumentException("unknown mode: " + mode);
            }
        } catch (Exception e) {
            System.err.println(e.getMessage());
            System.exit(1);
        }
    }

    private static String toHex(byte[] bytes) {
        StringBuilder sb = new StringBuilder(bytes.length * 2);
        for (byte b : bytes) {
            sb.append(String.format("%02x", b));
        }
        return sb.toString();
    }

    private static byte[] fromHex(String hex) {
        if (hex.length() % 2 != 0) {
            throw new IllegalArgumentException("invalid hex length");
        }
        byte[] out = new byte[hex.length() / 2];
        for (int i = 0; i < hex.length(); i += 2) {
            int hi = Character.digit(hex.charAt(i), 16);
            int lo = Character.digit(hex.charAt(i + 1), 16);
            if (hi < 0 || lo < 0) {
                throw new IllegalArgumentException("invalid hex character");
            }
            out[i / 2] = (byte) ((hi << 4) + lo);
        }
        return out;
    }
}
```

Command example (**use the absolute path** to the source file — do not use `java codec.java`; if the working directory is wrong, the JVM may print GBK errors to stdout and the app reports `invalid utf-8 sequence`):

```
java C:\Users\he_pe\redis\custom\codec.java
```

If `java` is not on PATH, use the full JDK path, for example:

```
"C:\Program Files\Java\jdk-21\bin\java.exe" C:\Users\he_pe\redis\custom\codec.java
```

## Troubleshooting

| Symptom                  | Likely cause                                                                                                                                             |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `invalid utf-8 sequence` | Script stdout is not UTF-8 (e.g. Chinese output on Windows); for Java: often a **relative** `codec.java` path — file not found, JVM prints GBK to stdout |
| python / java not found  | Interpreter not on PATH — use the **full path**                                                                                                          |
| Empty decode             | Script did not write to stdout on decode, or exit code is non-zero                                                                                       |
| encode hex errors        | Editor text has non-hex characters or odd length                                                                                                         |

The error message includes the **executed command** line for copy/paste into a terminal.

![](../../../public/images/codec/error.png)
