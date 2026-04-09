import { QRCodeSVG } from "qrcode.react";

type StyledQrCodeProps = {
  value: string;
  size?: number;
};

export function StyledQrCode({ value, size = 208 }: StyledQrCodeProps) {
  return (
    <div className="rounded-2xl bg-white p-4 shadow-[0_10px_30px_rgba(0,0,0,0.12)]">
      <QRCodeSVG
        value={value}
        size={size}
        bgColor="#ffffff"
        fgColor="#000000"
        includeMargin={false}
      />
    </div>
  );
}
