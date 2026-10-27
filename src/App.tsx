import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ZoneSetting {
  zone: string;
  mode: number;
  color: { r: number; g: number; b: number };
  color2: { r: number; g: number; b: number };
  brightness: number;
  speed: number;
}

interface KeyboardSetting {
  mode: number;
  color: { r: number; g: number; b: number };
  brightness: number;
}

interface RamSetting {
  color: { r: number; g: number; b: number };
  led_count: number;
}

interface Config {
  global_color: { r: number; g: number; b: number } | null;
  keyboard: KeyboardSetting | null;
  fans: ZoneSetting[];
  ram: RamSetting | null;
}

interface DeviceApplyResult {
  device: string;
  ok: boolean;
  message: string;
}

interface ApplyReport {
  results: DeviceApplyResult[];
}

interface HidDeviceInfo {
  path: string;
  vid: number;
  pid: number;
  name: string;
  serial: string | null;
}

interface RamModule {
  bus: number;
  address: number;
  led_count: number;
  controller: "corsair" | "ene";
}

interface I2cBusInfo {
  number: number;
  name: string;
}

interface SystemDevices {
  keyboards: HidDeviceInfo[];
  msi_boards: HidDeviceInfo[];
  ram_modules: RamModule[];
  smbus_buses: I2cBusInfo[];
  hidraw_devices: HidDeviceInfo[];
}

interface DeviceCounts {
  keyboards: number;
  ram_modules: number;
}

const toHex = (c: { r: number; g: number; b: number }) =>
  `#${[c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;

const fromHex = (hex: string) => {
  const s = hex.replace("#", "");
  return {
    r: parseInt(s.slice(0, 2), 16),
    g: parseInt(s.slice(2, 4), 16),
    b: parseInt(s.slice(4, 6), 16),
  };
};

function defaultConfig(): Config {
  const blue = { r: 0, g: 0, b: 255 };
  return {
    global_color: null,
    keyboard: { mode: 1, color: blue, brightness: 100 },
    fans: [],
    ram: { color: blue, led_count: 10 },
  };
}

export default function App() {
  const [config, setConfig] = useState<Config>(defaultConfig());
  const [devices, setDevices] = useState<DeviceCounts | null>(null);
  const [report, setReport] = useState<ApplyReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [globalHex, setGlobalHex] = useState("#0000ff");

  const refresh = useCallback(async () => {
    try {
      const cfg = await invoke<Config>("load_config");
      setConfig(cfg);
      const devs = await invoke<SystemDevices>("scan_devices");
      setDevices({
        keyboards: devs.keyboards.length,
        ram_modules: devs.ram_modules.length,
      });
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const doInvoke = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const apply = () =>
    doInvoke(async () => {
      const r = await invoke<ApplyReport>("apply", { config });
      setReport(r);
    });

  const applyKeyboard = () =>
    doInvoke(async () => {
      if (!config.keyboard) return;
      const r = await invoke<ApplyReport>("set_keyboard_color", {
        hex: toHex(config.keyboard.color),
      });
      setReport(r);
    });

  const applyRam = () =>
    doInvoke(async () => {
      if (!config.ram) return;
      const r = await invoke<ApplyReport>("set_ram_color", {
        hex: toHex(config.ram.color),
      });
      setReport(r);
    });

  const save = () =>
    doInvoke(async () => {
      await invoke("save_config", { config });
      setReport({ results: [{ device: "config", ok: true, message: "saved" }] });
    });

  const applyGlobal = () =>
    doInvoke(async () => {
      const r = await invoke<ApplyReport>("apply_all_color", { hex: globalHex });
      setReport(r);
    });

  return (
    <main className="app">
      <header>
        <h1>RGB Controller</h1>
        <div className="device-summary">
          {devices ? (
            <span>
              {devices.keyboards} keyboard(s) · {devices.ram_modules} RAM module(s)
            </span>
          ) : (
            <span>detecting devices…</span>
          )}
          <button onClick={() => void refresh()} disabled={busy}>
            Refresh
          </button>
        </div>
      </header>

      {error && <div className="banner error">{error}</div>}

      <section className="card">
        <h2>Keyboard + RAM</h2>
        <div className="row">
          <input
            type="color"
            value={globalHex}
            onChange={(e) => setGlobalHex(e.target.value)}
          />
          <button onClick={() => void applyGlobal()} disabled={busy}>
            Apply to all
          </button>
        </div>
      </section>

      <section className="card">
        <h2>Keyboard</h2>
        {config.keyboard ? (
          <div className="row">
            <input
              type="color"
              value={toHex(config.keyboard.color)}
              onChange={(e) =>
                setConfig((c) => ({
                  ...c,
                  keyboard: c.keyboard
                    ? { ...c.keyboard, color: fromHex(e.target.value) }
                    : c.keyboard,
                }))
              }
            />
            <button onClick={() => void applyKeyboard()} disabled={busy}>
              Apply
            </button>
          </div>
        ) : (
          <p>Not configured.</p>
        )}
      </section>

      <section className="card">
        <h2>RAM</h2>
        {config.ram ? (
          <div className="row">
            <input
              type="color"
              value={toHex(config.ram.color)}
              onChange={(e) =>
                setConfig((c) => ({
                  ...c,
                  ram: c.ram ? { ...c.ram, color: fromHex(e.target.value) } : c.ram,
                }))
              }
            />
            <button onClick={() => void applyRam()} disabled={busy}>
              Apply
            </button>
          </div>
        ) : (
          <p>Not configured.</p>
        )}
      </section>

      <section className="card actions">
        <button className="primary" onClick={() => void apply()} disabled={busy}>
          {busy ? "Working…" : "Apply all"}
        </button>
        <button onClick={() => void save()} disabled={busy}>
          Save config
        </button>
      </section>

      {report && (
        <section className="card">
          <h2>Result</h2>
          <ul className="report">
            {report.results.map((r, i) => (
              <li key={i} className={r.ok ? "ok" : "fail"}>
                [{r.ok ? "OK" : "FAIL"}] {r.device}: {r.message}
              </li>
            ))}
          </ul>
        </section>
      )}

      <footer>
        <p>
          Config saved at <code>~/.config/rgb-controller/config.json</code>. The{" "}
          <code>rgb-ctl apply</code> service re-applies colors at boot.
        </p>
      </footer>
    </main>
  );
}
