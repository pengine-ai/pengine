import * as Slider from "@radix-ui/react-slider";

type Props = {
  min: number;
  max: number;
  step: number;
  value: number;
  disabled: boolean;
  onValueChange: (bytes: number) => void;
  "aria-label": string;
};

export function SkillsContextBytesSlider({
  min,
  max,
  step,
  value,
  disabled,
  onValueChange,
  "aria-label": ariaLabel,
}: Props) {
  const clamped = Math.min(max, Math.max(min, value));

  return (
    <Slider.Root
      className="relative flex h-7 w-full touch-none select-none items-center"
      min={min}
      max={max}
      step={step}
      value={[clamped]}
      disabled={disabled}
      onValueChange={(next) => {
        const v = next[0];
        if (v !== undefined) onValueChange(v);
      }}
      aria-label={ariaLabel}
    >
      <Slider.Track className="relative h-2 w-full grow overflow-hidden rounded-full bg-white/[0.07] shadow-[inset_0_1px_2px_rgba(0,0,0,0.45)]">
        <Slider.Range className="absolute h-full rounded-full bg-linear-to-r from-cyan-500/35 via-cyan-400/55 to-teal-400/45" />
      </Slider.Track>
      <Slider.Thumb className="block size-4 shrink-0 rounded-full border-2 border-cyan-200/70 bg-[var(--bg2)] shadow-[0_0_0_1px_rgba(0,0,0,0.35),0_2px_10px_rgba(34,211,238,0.35)] outline-none transition-[box-shadow,transform] hover:border-cyan-200 hover:shadow-[0_0_0_1px_rgba(0,0,0,0.35),0_2px_14px_rgba(34,211,238,0.5)] focus-visible:ring-2 focus-visible:ring-cyan-400/60 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] disabled:pointer-events-none disabled:opacity-40 data-[disabled]:cursor-not-allowed" />
    </Slider.Root>
  );
}
