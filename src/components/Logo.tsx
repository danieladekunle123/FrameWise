// The FrameWise logo, drawn from the kit's master SVGs (logo/kit/masters).
// Colours come from --logo-* tokens, which follow the light and "-on-dark" masters and are
// deliberately separate from --accent, so the logo never changes with the Windows accent setting.
// Don't edit these paths by hand: change logo/kit/src/build_kit.py and copy the new masters.

const FRAME =
  "M36 220V52A16 16 0 0 1 52 36H112V64H64V114H96V142H64V220ZM144 36H204A16 16 0 0 1 220 52V112H192V64H144ZM144 220V192H192V144H220V204A16 16 0 0 1 204 220Z";
const TARGET = "M121 115H135A6 6 0 0 1 141 121V135A6 6 0 0 1 135 141H121A6 6 0 0 1 115 135V121A6 6 0 0 1 121 115Z";

/** Small cut, for 32 px and below. */
const FRAME_SMALL =
  "M28 228V38A10 10 0 0 1 38 28H112V68H68V108H90V148H68V228ZM144 28H218A10 10 0 0 1 228 38V112H188V68H144ZM144 228V188H188V144H228V218A10 10 0 0 1 218 228Z";
const TARGET_SMALL = "M118 113H138A5 5 0 0 1 143 118V138A5 5 0 0 1 138 143H118A5 5 0 0 1 113 138V118A5 5 0 0 1 118 113Z";

const WORDMARK =
  "M0 200V52a12 12 0 0 1 12-12h88v26h-74v42h62v26h-62v66zM116 200V110a30 30 0 0 1 30-30h46v26h-44a6 6 0 0 0-6 6v88zM236 80h66v120h-66a30 30 0 0 1-30-30v-60a30 30 0 0 1 30-30zM238 106h38v68h-38a6 6 0 0 1-6-6v-56a6 6 0 0 1 6-6zM324 200V110a30 30 0 0 1 30-30h90a30 30 0 0 1 30 30v90h-26v-88a6 6 0 0 0-6-6h-24a6 6 0 0 0-6 6v88h-26v-88a6 6 0 0 0-6-6h-24a6 6 0 0 0-6 6v88zM494 170V110a30 30 0 0 1 30-30h36a30 30 0 0 1 30 30v42h-70v16a6 6 0 0 0 6 6h64v26h-66a30 30 0 0 1-30-30zM526 106h32a6 6 0 0 1 6 6v14h-44v-14a6 6 0 0 1 6-6zM612 40h26v128a6 6 0 0 0 6 6h37a6 6 0 0 0 6-6v-88h26v88a6 6 0 0 0 6 6h37a6 6 0 0 0 6-6v-128h26v130a30 30 0 0 1-30 30h-116a30 30 0 0 1-30-30zM810 80h26v120h-26zM888 80h66v26h-64a6 6 0 0 0-6 6v9a6 6 0 0 0 6 6h34a30 30 0 0 1 30 30v13a30 30 0 0 1-30 30h-66v-26h64a6 6 0 0 0 6-6v-9a6 6 0 0 0-6-6h-34a30 30 0 0 1-30-30v-13a30 30 0 0 1 30-30zM974 170V110a30 30 0 0 1 30-30h36a30 30 0 0 1 30 30v42h-70v16a6 6 0 0 0 6 6h64v26h-66a30 30 0 0 1-30-30zM1006 106h32a6 6 0 0 1 6 6v14h-44v-14a6 6 0 0 1 6-6z";
const WORDMARK_DOT = "M816 40h14a6 6 0 0 1 6 6v14a6 6 0 0 1-6 6h-14a6 6 0 0 1-6-6v-14a6 6 0 0 1 6-6z";

type Props = { className?: string; title?: string };

/** Horizontal lockup (primary). Minimum 120 px wide. */
export function LogoHorizontal({ className, title = "FrameWise" }: Props) {
  return (
    <svg className={className} viewBox="0 0 1046 256" role="img" aria-label={title}>
      <path fill="var(--logo-frame)" d={FRAME} />
      <path fill="var(--logo-target)" d={TARGET} />
      <g transform="translate(272 45.2) scale(0.69)">
        <path fill="var(--logo-ink)" fillRule="evenodd" d={WORDMARK} />
        <path fill="var(--logo-target)" d={WORDMARK_DOT} />
      </g>
    </svg>
  );
}

/** Symbol. Uses the small cut automatically at 32 px and below. */
export function LogoSymbol({ size, className, title = "FrameWise" }: Props & { size: number }) {
  const small = size <= 32;
  return (
    <svg className={className} width={size} height={size} viewBox="0 0 256 256" role="img" aria-label={title}>
      <path fill="var(--logo-frame)" d={small ? FRAME_SMALL : FRAME} />
      <path fill="var(--logo-target)" d={small ? TARGET_SMALL : TARGET} />
    </svg>
  );
}
