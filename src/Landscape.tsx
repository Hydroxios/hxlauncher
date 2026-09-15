export default function Landscape({ small = false }: { small?: boolean }) {
  const trees = Array.from({ length: small ? 20 : 65 }, (_, i) => {
    const x = (i * 137 + 71) % 1100;
    const y = 270 + ((i * 83) % 240);
    return { x, y, size: 0.4 + y / 570 };
  }).sort((a, b) => a.y - b.y);
  return (
    <svg
      className="landscape"
      viewBox="0 0 1100 540"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
    >
      <defs>
        <linearGradient id={small ? "sky-s" : "sky"} x2="0" y2="1">
          <stop stopColor="#657d7a" />
          <stop offset="1" stopColor="#bbbd94" />
        </linearGradient>
        <linearGradient id={small ? "water-s" : "water"} x2="0" y2="1">
          <stop stopColor="#759f96" />
          <stop offset="1" stopColor="#304f49" />
        </linearGradient>
      </defs>
      <rect
        width="1100"
        height="540"
        fill={`url(#${small ? "sky-s" : "sky"})`}
      />
      <circle cx="812" cy="117" r="43" fill="#e5deb1" opacity=".65" />
      <g fill="#e4e4cd" opacity=".22">
        <path d="M570 66h133v10h60v14H560V79h-54V66z" />
        <path d="M820 172h177v13h58v13H798v-13h22z" />
        <path d="M80 105h123v14h75v13H45v-13h35z" />
      </g>
      <path
        d="M0 256V209h70v-40h66v-37h41v-29h55v29h31v45h50v-30h42v-29h47v43h39v43h79v-26h43v-42h52v-36h34v-29h40v30h34v49h33v26h81v-22h49v-51h57v-23h35v-42h42v34h34v30h39v41h57v60z"
        fill="#637b70"
      />
      <path
        d="M0 316V243h89v-30h69v38h51v-8h37v-45h47v-37h43v27h38v64h54v27h66v-43h43v-49h42v36h62v37h49v-35h57v-43h38v-26h43v43h35v53h61v-30h67v31h47v-53h69v126z"
        fill="#425e50"
      />
      <path
        d="M0 342V282h103v-24h101v31h145v32h102v36h119v-25h76v-45h102v-35h93v-21h94v28h67v33h98v248H0z"
        fill="#304d3b"
      />
      <path
        d="M600 296h54v20h-39v21h61v20h-53v24h60v23h-31v28h83v27h72v27h105v54H388v-39h91v-30h54v-28h-20v-23h43v-32h-28v-30h49v-31h23z"
        fill={`url(#${small ? "water-s" : "water"})`}
      />
      <g fill="#b1c2a2" opacity=".25">
        <path d="M565 383h63v3h-63zM552 432h91v3h-91zM575 462h122v4H575zM494 495h240v3H494zM601 342h49v2h-49z" />
      </g>
      <path
        d="M0 340h65v-25h70v38h114v33h92v30h60v44h57v80H0z"
        fill="#263f30"
      />
      <path
        d="M820 359h48v-40h85v-28h58v25h89v224H748v-45h38v-48h34z"
        fill="#253e2e"
      />
      {trees
        .filter((t) => t.x < 470 || t.x > 800 || t.y < 310)
        .map((t, i) => (
          <g key={i} transform={`translate(${t.x} ${t.y}) scale(${t.size})`}>
            <path d="M-3 0h6v28h-6z" fill="#342f26" />
            <path
              d="M-22 6h44V-8H15v-14H9v-15H3v-14h-6v14h-6v15h-6v14h-7z"
              fill={i % 3 === 0 ? "#496345" : "#213f31"}
            />
            <path
              d="M-22 6H0v-57h-3v14h-6v15h-6v14h-7z"
              fill="#172f27"
              opacity=".35"
            />
          </g>
        ))}
      <g transform="translate(886 351)">
        <path d="M0 0h70v46H0z" fill="#8e8160" />
        <path d="M-7 0v-9H2v-10h10v-9h45v9h10v10h10V0z" fill="#423d2f" />
        <path d="M8 10h14v14H8zm38 0h14v14H46z" fill="#d4ba6c" />
        <path d="M28 19h14v27H28z" fill="#443d2e" />
        <path d="M58-25v-24h10v33" fill="#696650" />
      </g>
      <g fill="#98a265">
        <rect x="107" y="432" width="5" height="6" />
        <rect x="232" y="465" width="5" height="7" />
        <rect x="910" y="474" width="5" height="6" />
      </g>
    </svg>
  );
}
