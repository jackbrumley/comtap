# com0com driver files

Put the **signed x64 com0com 2.2.2.0** files here, and `cargo build` embeds them into `comtap.exe`:

```
cncport.inf  com0com.cat  com0com.inf  com0com.sys  comport.inf  setup.dll  setupc.exe
```

Where to get them:

1. Download `com0com-2.2.2.0-x64-fre-signed.zip` from https://sourceforge.net/projects/com0com/files/com0com/2.2.2.0/
2. The zip holds an installer exe. Open it with 7-Zip (Open archive), or install it once on a dev PC and copy the files from `C:\Program Files (x86)\com0com`.
3. Also keep the `license.txt` from the same package. com0com is GPL, so its licence ships with ComTap.

The binaries are git-ignored. Only this README is committed.
