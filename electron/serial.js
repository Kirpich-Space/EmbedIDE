const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

let pythonScript = '';
let activeProcess = null;

function getPythonScript() {
  if (pythonScript) return pythonScript;

  pythonScript = path.join(require('os').tmpdir(), 'embedide_serial_mon.py');
  const code = `#!/usr/bin/env python3
import sys, time, json, threading

try:
    import serial
    import serial.tools.list_ports
except ImportError:
    print(json.dumps({"type":"error","msg":"pyserial not installed. Run: pip3 install pyserial"}))
    sys.exit(1)

def list_ports():
    ports = serial.tools.list_ports.comports()
    result = []
    for p in sorted(ports):
        result.append({"device": p.device, "description": p.description})
    return result

if __name__ == "__main__":
    args = sys.argv[1:]

    if args and args[0] == "list":
        print(json.dumps(list_ports()))
        sys.exit(0)

    if len(args) >= 2:
        port = args[0]
        baud = int(args[1])

        try:
            ser = serial.Serial(port, baud, timeout=0.1)
            print(json.dumps({"type":"connected","port":port,"baud":baud}))
            sys.stdout.flush()

            def read_loop():
                while True:
                    try:
                        data = ser.readline()
                        if data:
                            print(json.dumps({"type":"data","text":data.decode('utf-8','replace').rstrip()}))
                            sys.stdout.flush()
                    except:
                        break

            t = threading.Thread(target=read_loop, daemon=True)
            t.start()

            for line in sys.stdin:
                line = line.strip()
                if line == "__close__":
                    break
                if line:
                    ser.write((line + "\\n").encode('utf-8'))

        except Exception as e:
            print(json.dumps({"type":"error","msg":str(e)}))
        finally:
            try: ser.close()
            except: pass
    else:
        print(json.dumps({"type":"error","msg":"Usage: serial_mon.py <port> <baud>"}))
        sys.exit(1)
`;
  fs.writeFileSync(pythonScript, code);
  return pythonScript;
}

function listSerialPorts() {
  return new Promise((resolve, reject) => {
    const script = getPythonScript();
    const proc = spawn('python3', [script, 'list']);
    let output = '';
    proc.stdout.on('data', d => output += d.toString());
    proc.on('close', code => {
      if (code === 0) {
        try { resolve(JSON.parse(output)) }
        catch { resolve([]) }
      } else resolve([]);
    });
    proc.on('error', () => resolve([]));
  });
}

function connectSerial(port, baud, onData, onError) {
  disconnectSerial();
  const script = getPythonScript();
  const proc = spawn('python3', [script, port, String(baud)]);
  activeProcess = proc;

  proc.stdout.on('data', (data) => {
    const lines = data.toString().trim().split('\n');
    for (const line of lines) {
      try {
        const msg = JSON.parse(line);
        if (msg.type === 'data') onData?.(msg.text);
        else if (msg.type === 'connected') onData?.(`Connected to ${msg.port} @ ${msg.baud} baud`);
        else if (msg.type === 'error') onError?.(msg.msg);
      } catch {}
    }
  });

  proc.stderr.on('data', (data) => onError?.(data.toString()));
  proc.on('error', (err) => onError?.(err.message));
  proc.on('close', () => { activeProcess = null; onData?.('Disconnected'); });

  return {
    send: (text) => { if (activeProcess) activeProcess.stdin.write(text + '\n'); },
    disconnect: () => { if (activeProcess) { activeProcess.stdin.write('__close__\n'); activeProcess = null; } },
  };
}

function disconnectSerial() {
  if (activeProcess) {
    try { activeProcess.stdin.write('__close__\n'); } catch {}
    try { activeProcess.kill(); } catch {}
    activeProcess = null;
  }
}

module.exports = { getPythonScript, listSerialPorts, connectSerial, disconnectSerial };
