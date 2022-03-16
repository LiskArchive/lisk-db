const { Database, InMemoryDatabase, Batch, StateDB } = require("../index");
const crypto = require('crypto');
const childProcess = require('child_process');
const path = require('path');

const getRandomBytes = () => crypto.randomBytes(32);

(async () => {

    const reader = childProcess.fork(path.join(__dirname, 'reader.js'));
    reader.send({ type: 'init', count: 1 })
    await new Promise(resolve => {
        reader.on('message', msg => {
            if (msg.type === 'ready') {
                resolve();
            }
        });
    });
    const reader2 = childProcess.fork(path.join(__dirname, 'reader.js'));
    reader2.send({ type: 'init', count: 2 })
    await new Promise(resolve => {
        reader2.on('message', msg => {
            if (msg.type === 'ready') {
                resolve();
            }
        });
    });
    const writer = childProcess.fork(path.join(__dirname, 'writer.js'));
    writer.on('message', msg => {
        // console.log(msg);
        reader.send(msg);
        reader2.send(msg);
    });
    console.log('forked');
})()