# IPK fixtures

Two packages holding the same trivial web app, built two ways. `ipk.rs` reads
both, so the package-level warnings are tested against real files.

The app source is three files in one directory:

```
com.example.fixture/
  appinfo.json   {"id":"com.example.fixture","version":"1.0.0","vendor":"webosbrew",
                  "type":"web","main":"index.html","title":"Fixture","icon":"icon.png"}
  index.html     <html><body><script>var greeting = "hi";</script></body></html>
  icon.png       the 8-byte PNG signature, enough for a file that exists
```

## `ares_packaged.ipk`

The output of the official packager, unedited:

```
ares-package -o <outdir> com.example.fixture
```

Its control file carries `Installed-Size` and both `webOS-*` fields, which is
what tells a real package from one somebody assembled.

## `hand_rolled.ipk`

The same app, put together by hand, with the two faults this warns about:

* the control file stops after `Description` — no `Installed-Size`, no
  `webOS-Package-Format-Version`, no `webOS-Packager-Version`
* the control archive also holds a `postinst` and a `prerm`

It follows the shape of a real submission that was turned away for this
(webosbrew/apps-repo#218). The `ar` and `tar` members are written directly, with
mtime 0 and `root:root` ownership so the file stays byte-stable if it is rebuilt.
