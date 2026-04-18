import * as Select from "@radix-ui/react-select";
import { useMemo } from "react";

const HOURS = Array.from({ length: 24 }, (_, i) => i);
const MINUTES = Array.from({ length: 60 }, (_, i) => i);

const triggerClass =
  "inline-flex h-8 w-[4.25rem] shrink-0 items-center justify-between gap-1 rounded-md border border-white/10 bg-black/30 px-2 font-mono text-[11px] text-white outline-none focus:border-cyan-300/40 data-[state=open]:border-cyan-300/40";

const contentClass =
  "z-[100] max-h-52 overflow-hidden rounded-md border border-white/12 bg-[#1a1a22] shadow-xl";

const viewportClass = "max-h-52 p-0.5";

const itemClass =
  "relative flex cursor-pointer select-none items-center rounded px-2 py-1.5 font-mono text-[11px] text-white/90 outline-none data-[disabled]:pointer-events-none data-[disabled]:opacity-40 data-[highlighted]:bg-cyan-300/15 data-[highlighted]:text-cyan-100";

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

function SelectChevron() {
  return (
    <span aria-hidden className="text-[10px] leading-none text-white/45">
      {"\u25BC"}
    </span>
  );
}

export type CronDailyLocalTimePickerProps = {
  hour: number;
  minute: number;
  onChange: (hour: number, minute: number) => void;
  disabled?: boolean;
};

export function CronDailyLocalTimePicker({
  hour,
  minute,
  onChange,
  disabled,
}: CronDailyLocalTimePickerProps) {
  const timeZone = useMemo(() => {
    try {
      return Intl.DateTimeFormat().resolvedOptions().timeZone;
    } catch {
      return "local";
    }
  }, []);

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex flex-wrap items-center gap-1.5">
        <Select.Root
          value={String(hour)}
          onValueChange={(v) => {
            const h = Number.parseInt(v, 10);
            if (Number.isInteger(h)) onChange(h, minute);
          }}
          disabled={disabled}
        >
          <Select.Trigger aria-label="Hour" className={triggerClass} disabled={disabled}>
            <Select.Value />
            <Select.Icon className="text-white/40">
              <SelectChevron />
            </Select.Icon>
          </Select.Trigger>
          <Select.Portal>
            <Select.Content className={contentClass} position="popper" sideOffset={4}>
              <Select.Viewport className={viewportClass}>
                {HOURS.map((h) => (
                  <Select.Item key={h} value={String(h)} className={itemClass}>
                    <Select.ItemText>{pad2(h)}</Select.ItemText>
                  </Select.Item>
                ))}
              </Select.Viewport>
            </Select.Content>
          </Select.Portal>
        </Select.Root>

        <span className="font-mono text-[11px] text-white/50">:</span>

        <Select.Root
          value={String(minute)}
          onValueChange={(v) => {
            const m = Number.parseInt(v, 10);
            if (Number.isInteger(m)) onChange(hour, m);
          }}
          disabled={disabled}
        >
          <Select.Trigger aria-label="Minute" className={triggerClass} disabled={disabled}>
            <Select.Value />
            <Select.Icon className="text-white/40">
              <SelectChevron />
            </Select.Icon>
          </Select.Trigger>
          <Select.Portal>
            <Select.Content className={contentClass} position="popper" sideOffset={4}>
              <Select.Viewport className={viewportClass}>
                {MINUTES.map((m) => (
                  <Select.Item key={m} value={String(m)} className={itemClass}>
                    <Select.ItemText>{pad2(m)}</Select.ItemText>
                  </Select.Item>
                ))}
              </Select.Viewport>
            </Select.Content>
          </Select.Portal>
        </Select.Root>

        <span className="font-mono text-[10px] text-white/40">{timeZone}</span>
      </div>
      <p className="font-mono text-[9px] leading-snug text-white/35">
        Runs once per day at this clock time on this device (same timezone as the system clock).
      </p>
    </div>
  );
}
