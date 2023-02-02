#!/usr/bin/env node

/**
 * Reference: https://github.com/IronCoreLabs/recrypt-node-binding/blob/main/publish.js
 */

 const fs = require("fs");
 const path = require("path");
 const shell = require("shelljs");
 
 //Fail this script if any of these commands fail
 shell.set("-e");
 //Ensure that our directory is set to the root of the repo
 const rootDirectory = path.dirname(process.argv[1]);
 shell.cd(rootDirectory);
 const shouldPublish = process.argv.slice(2).indexOf("--publish") !== -1;
 const canaryFlagIndex = process.argv.slice(2).indexOf("--canary");
 const canarySHA = canaryFlagIndex !== -1 ? process.argv.slice(2)[canaryFlagIndex + 1] : undefined;
 const registryFlagIndex = process.argv.slice(2).indexOf("--registry");
 const registry = registryFlagIndex !== -1 ? process.argv.slice(2)[registryFlagIndex + 1] : undefined;
 
 // Cleanup any previous Rust builds, update deps, and compile
 shell.exec("yarn install --ignore-scripts");
 shell.exec("yarn clean");
 shell.exec("yarn build-release");
 
 // As long as rustc's output is consistent, this should be fine
 const host = shell
     .exec("rustc -vV")
     .toString()
     .split("\n")
     .find((line) => line.startsWith("host:"))
     .split(" ")[1];
 const cargoTarget = process.env.CARGO_BUILD_TARGET;
 // Skip running tests with a cross compiled binary, we know they'll fail to run
 if (host === cargoTarget || cargoTarget === "" || cargoTarget === undefined) {
     shell.exec("yarn test:node");
     shell.exec("yarn test:rust");
 }

//Add a NPM install script to the package.json that we push to NPM so that when consumers pull it down it
 //runs the expected node-pre-gyp step.
 const originalPackageJSON = require("./package.json");
 if (canarySHA) {
    originalPackageJSON.version = `${originalPackageJSON.version}-${canarySHA}`;
 }
 originalPackageJSON.scripts.install = "node install";
 fs.writeFileSync("./package.json", JSON.stringify(originalPackageJSON, null, 2));
 
 //Use a fully qualified path to pre-gyp binary for Windows support
 const cwd = shell.pwd().toString();
 const replacementArch = process.env.PRE_GYP_ARCH ? `--target_arch=${process.env.PRE_GYP_ARCH}` : "";
 const replacementPlatform = process.env.PRE_GYP_PLATFORM ? `--target_platform=${process.env.PRE_GYP_PLATFORM}` : "";
 shell.exec(`${cwd}/node_modules/@mapbox/node-pre-gyp/bin/node-pre-gyp package ${replacementArch} ${replacementPlatform}`);
 var tgz = shell.exec("find ./build -name *.tar.gz");
 shell.cp(tgz, "./bin-package/");
 
 var publishCmd = "echo 'Skipping publishing to npm...'"
 if (shouldPublish) {
     publishCmd = "npm publish --access public";
     // If we're publishing a branch build or prerelease like "1.2.3-pre.4", use "--tag next".
     if (canarySHA) {
         publishCmd += " --tag next";
     }
     if (registry) {
         publishCmd += ` --registry ${registry}`;
     }
 }
 shell.exec(publishCmd);
 
 shell.echo("publish.js COMPLETE");