const { Database, Batch } = require('../index');
const bunyan = require('bunyan');
const childProcess = require('child_process');

const crypto = require('crypto');
const path = require('path');

const getRandomBytes = () => crypto.randomBytes(32);

const logger = bunyan.createLogger({
    name: 'benchmark',
    streams: [
        {
            level: 'info',
            stream: process.stdout            // log INFO and above to stdout
        },
        {
            level: 'info',
            path: __dirname + '/benchmark.log'  // log ERROR and above to a file
        }
    ]
});





(async () => {
    const db = new Database('./.tmp');
    const handleMessage = (channel, msg) => {
        if (msg.type === 'get') {
            db.get(Buffer.from(msg.key, 'hex'))
                .then(val => {
                    channel.send({
                        id: msg.id,
                        type: 'get_resp',
                        value: val.toString('hex'),
                    });
                })
                .catch(err => {
                    channel.send({
                        id: msg.id,
                        type: 'get_resp',
                        err,
                    });
                });
            return;
        }

        if (msg.type === 'set') {
            const batch = new Batch();
            for (const kv of msg.data) {
                batch.set(Buffer.from(kv.key, 'hex'), Buffer.from(kv.value, 'hex'));
            }
            db.write(batch)
                .then(val => {
                    channel.send({
                        id: msg.id,
                        type: 'set_resp',
                        err: undefined,
                    });
                })
                .catch(err => {
                    channel.send({
                        id: msg.id,
                        type: 'set_resp',
                        err,
                    });
                });
        }
    }


    const reader = childProcess.fork(path.join(__dirname, 'reader.js'));
    reader.send({ type: 'init', count: 1 })
    const reader2 = childProcess.fork(path.join(__dirname, 'reader.js'));
    reader2.send({ type: 'init', count: 2 })
    reader.on('message', msg => {
        handleMessage(reader, msg);
    });
    reader2.on('message', msg => {
        handleMessage(reader2, msg);
    });
    console.log('forked');
})()