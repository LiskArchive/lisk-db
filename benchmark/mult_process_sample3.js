const { Database, Batch } = require('../index');
const bunyan = require('bunyan');
const childProcess = require('child_process');

const crypto = require('crypto');
const path = require('path');

const getRandomBytes = (size) => crypto.randomBytes(size);

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
    const handleMessage = (channel, msg) => {
        if (msg.type === 'get') {
            logger.debug(msg);
            channel.send({
                id: msg.id,
                type: 'get_resp',
                value: getRandomBytes(100),
            });
            return;
        }
    }


    const reader = childProcess.fork(path.join(__dirname, 'engine.js'));
    reader.send({ type: 'init', count: 1 })
    // const reader2 = childProcess.fork(path.join(__dirname, 'reader.js'));
    // reader2.send({ type: 'init', count: 2 })
    reader.on('message', msg => {
        handleMessage(reader, msg);
    });
    // reader2.on('message', msg => {
    //     handleMessage(reader2, msg);
    // });
    console.log('forked');
})()