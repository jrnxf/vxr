# Security

## Report a problem

Report a security problem in private, not in a public issue.

1. Open the repository's **Security** tab.
2. Choose **Report a vulnerability**.
3. Describe the problem, the version (`vxr --version`, or the date you used the site) and the steps to see it.

Please give time for a fix before you publish.

## What is in scope

- The desktop app and its command line, including `vxr mcp`, `vxr bridge` and the files they read and write.
- Opening a file from a stranger: a `.vox` file, a brick scene file, a palette or an image.
- The `<voxel-scene>` embed viewer.
- The site and the account service at vxr.jrnxf.co: sign-in, sessions and stored projects.

## What vxr does with your data

- API keys for the assistant stay on your machine: in the OS keychain on the desktop, and in the browser tab (or that browser, if you ask it to remember) on the web. They go only to the provider you chose.
- `vxr mcp --root <dir>` reaches only the files under that folder. It has no network and no shell tool.
- `vxr bridge` listens on 127.0.0.1 only, answers only the vxr editor's page, and needs a pairing code.
