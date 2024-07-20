/**
 * @param {string} b64 
 */
async function b64StrToBytes(b64) {
    const res = await fetch('data:application/octet-stream;base64,' + b64)
    return new Uint8Array(await res.arrayBuffer())
}

/**
 * @param {ArrayBuffer} bytes
 * @returns {Promise<string>}
 */
async function bytesToB64Str(bytes) {
    const url = await new Promise((resolve, reject) => {
        const reader = Object.assign(new FileReader(), {
            onload: () => resolve(reader.result),
            onerror: () => reject(reader.error),
        })
        reader.readAsDataURL(new File([bytes], "", { type: "application/octet-stream" }))
    })
    return url.split(',')[1]
}

/**
 * @param {number} u64 
 */
function u64ToArrayBuffer(u64) {
    const buf = new ArrayBuffer(8)
    const view = new DataView(buf)
    view.setBigUint64(0, BigInt(u64), false)
    return buf
}

/**
 * @typedef {{id: string, seed: string}} AuthResponse
 * @typedef {{code: string}} ErrorResponse
 */
class Session {
    constructor() {
        this.privateKey = localStorage.getItem('private-key') ?? ''
        this.key = undefined
        this.seed = new Uint8Array()
        this.authBuf = new ArrayBuffer(0)
    }

    /**
     * @param {string} privateKey Base64ed
     */
    setPrivateKey(privateKey) {
        this.privateKey = privateKey
        this.key = undefined
        this.seed = new Uint8Array()
        this.authBuf = new ArrayBuffer(0)
        localStorage.setItem('private-key', privateKey)
    }

    async genAuthRequest() {
        const keyBytes = await b64StrToBytes(this.privateKey)
        this.key = await window.crypto.subtle.importKey("raw", keyBytes, { name: "HMAC", hash: "SHA-256" }, true, [
            "sign",
        ])
        const content = u64ToArrayBuffer(Number(new Date()))
        const sign = await window.crypto.subtle.sign({ name: "HMAC", hash: "SHA-256" }, this.key, content)
        const result = await bytesToB64Str(sign)
        return { timestamp: await bytesToB64Str(content), auth: result }
    }

    async refreshToken() {
        const authRequest = await this.genAuthRequest()
        const res = await fetch('session', {
            method: 'POST',
            body: JSON.stringify(authRequest),
            headers: { 'Content-Type': 'application/json' }
        })
        if (res.ok) {
            const authRes = /** @type {AuthResponse} */ (await res.json())
            const idInput = await b64StrToBytes(authRes.id)
            this.seed = await b64StrToBytes(authRes.seed)
            this.authBuf = new ArrayBuffer(idInput.byteLength + 32)
            new Uint8Array(this.authBuf, 0, idInput.byteLength).set(idInput)
        } else {
            const errorText = /** @type {ErrorResponse} */ (await res.text())
            /** @type {ErrorResponse} */
            let err
            try {
                err = JSON.parse(errorText)
            } catch (ex) {
                throw new Error(`Invalid Auth Response ${res.status} ${res.statusText}: ${errorText}`)
            }
            throw new Error('Auth failed: ' + err.code)
        }
    }

    async genHeader() {
        while (!this.key) {
            await this.refreshToken()
        }
        const sign = await window.crypto.subtle.sign({ name: "HMAC", hash: "SHA-256" }, this.key, this.seed)
        new Uint8Array(this.authBuf, 16, 32).set(new Uint8Array(sign))
        const result = await bytesToB64Str(this.authBuf)
        let c = 1
        for (let i = 0; i < 64; i++) {
            c += this.seed[i]
            this.seed[i] = c & 0xff
            c >>= 8
        }
        return result
    }

    /**
     * @param {CmdName} name 
     */
    async runCommand(name) {
        const authHeader = await this.genHeader()
        const res = await fetch('cmd/' + name, {
            method: 'POST',
            headers: {
                'content-type': 'application/json',
                'session-verify': authHeader
            }
        })
        if (!res.ok) {
            const errorText = /** @type {ErrorResponse} */ (await res.text())
            /** @type {ErrorResponse} */
            let err
            try {
                err = JSON.parse(errorText)
            } catch (ex) {
                throw new Error(`Invalid Cmd Response ${res.status} ${res.statusText}: ${errorText}`)
            }
            throw new Error('Cmd failed: ' + err.code)
        }
    }
}

const Commands =
    /** @type {const} */
    (['play_pause', 'prev_track', 'next_track', 'volume_up', 'volume_down'])
/** @typedef {typeof Commands[number]} CmdName */

/** @type {{name: CmdName, resolve: () => {}[]}[]} */
const cmdRequests = []
let notifyNewSession = () => { }

async function sessionLoop() {
    console.log('loaded v1')
    let session = new Session()
    const $inputPrivateKey = document.getElementById('input-private-key')
    $inputPrivateKey.value = session.privateKey
    $inputPrivateKey.addEventListener('change', e => {
        /** @type {HTMLInputElement} */
        const $el = e.target
        session.setPrivateKey($el.value)
    })
    document.querySelectorAll('#panel-controller .button-controller').forEach($el => {
        $el.addEventListener('click', async e => {
            const cmd = $el.getAttribute('data-cmd-name')
            if (!Commands.includes(cmd)) {
                alert('Invalid command: ' + cmd)
                return
            }
            cmdRequests.push({
                name: cmd,
                resolve: () => { }
            })
            notifyNewSession()
        })
    })
    while (true) {
        const cmd = cmdRequests.shift()
        if (!cmd) {
            await new Promise(resolve => notifyNewSession = resolve)
            continue
        }

        try {
            await session.runCommand(cmd.name)
        } catch (e) {
            session = new Session()
            alert('Error: ' + e)
            console.error(e)
        } finally {
            cmd.resolve()
        }
    }
}

sessionLoop()
