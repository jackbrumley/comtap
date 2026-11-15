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

The binaries are git-ignored. Only this README is committed.
