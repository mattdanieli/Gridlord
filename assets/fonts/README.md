# Fonts

| File | Font | License |
|---|---|---|
| `Inter-Regular.ttf`, `Inter-SemiBold.ttf` | [Inter](https://rsms.me/inter/) | [SIL OFL 1.1](OFL-Inter.txt) |
| `NotoSansSC-UI.otf` | [Noto Sans SC](https://github.com/notofonts/noto-cjk), subset | [SIL OFL 1.1](OFL-NotoSansSC.txt) |

`NotoSansSC-UI.otf` contains only the Chinese characters used by the interface (the `ZH` table in
`src/i18n.rs`), so the executable stays small. After changing the Chinese translations, regenerate it:

```bash
# all non-Latin characters of the Chinese table
python3 -c "import re;s=open('src/i18n.rs',encoding='utf-8').read();t=s[s.index('const ZH'):s.index('const DE')];print(''.join(sorted({c for c in t if ord(c)>0x2E7F})))" > zh-chars.txt
pyftsubset NotoSansSC-Regular.otf --text-file=zh-chars.txt --output-file=assets/fonts/NotoSansSC-UI.otf \
  --layout-features='*' --no-hinting --desubroutinize
```

Sample names in Chinese, Japanese or Korean are shown with the system CJK font (Microsoft YaHei on
Windows, PingFang on macOS) when available.
