const shell = require("shelljs");
 
shell.exec("npx node-pre-gyp install");
if (shell.error()) {
    shell.exec("yarn install --ignore-scripts");
    shell.exec("yarn clean");
    shell.exec("yarn build-release");
}
