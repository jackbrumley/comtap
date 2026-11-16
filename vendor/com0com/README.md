# com0com driver files

Put the **signed x64 com0com 2.2.2.0** files here, and `cargo build` embeds them into `comtap.exe`:

```
setupc.exe  setup.dll  com0com.inf  com0com.sys  com0com.cat  ReadMe.txt
```

How to get them, without running the com0com installer:

```powershell
# com0com-2.2.2.0-x64-fre-signed.zip from https://sourceforge.net/projects/com0com/files/com0com/2.2.2.0/
& "C:\Program Files\7-Zip\7z.exe" x com0com-2.2.2.0-x64-fre-signed.zip -oc0c-zip
& "C:\Program Files\7-Zip\7z.exe" x c0c-zip\com0com-2.2.2.0-x64-fre-signed\setup.exe -oc0c-inst
copy c0c-inst\setupc.exe, c0c-inst\setup.dll, c0c-inst\com0com.*, c0c-inst\ReadMe.txt vendor\com0com\
```

- `com0com.cat` is signed by "Steven William Hatchett" (2011, timestamped). It signs `com0com.sys`.
- `ReadMe.txt` carries com0com's GPL notice, and ships with ComTap.


SHA-256 of the committed files (com0com 2.2.2.0 signed x64, unchanged):

```
0cede12ac635cbe4cd460d420caa3c9c41fb6bc7642fd81e053ff67db08e7694 *com0com.cat
5612e8834798a690b72cfd04828123f5d963dd105da9d90af8fa5bddf844269f *com0com.inf
bca2c9f4cb587242470cf07ea2a7e0c33de8c8026c06f37a181ace1992d6108b *com0com.sys
b7f893a05e4961d78f3b5c6c14d03963d6b72aeacdafb1f3aeb0f6d70defa431 *setup.dll
dd92f0980a2d659fd733b797966d6bf51d9f8870a5ca1ab4322dbedd62823a61 *setupc.exe
26ce4d037cf8175a7c211325f138ab33eddf58b977e2661a0196a0df5d8e93a6 *ReadMe.txt
```
