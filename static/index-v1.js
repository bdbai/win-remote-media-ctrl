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
    (['play_pause', 'prev_track', 'next_track', 'volume_up', 'volume_down', 'like'])
/** @typedef {typeof Commands[number]} CmdName */

/** @type {{name: CmdName, resolve: () => {}[]}[]} */
const cmdRequests = []
let notifyNewCommand = () => { }

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
    /**
     * @param {Element} $el 
     * @returns {(e: Event) => void}
     */
    const commandHandler = $el => e => {
        const cmd = $el.getAttribute('data-cmd-name')
        if (!Commands.includes(cmd)) {
            alert('Invalid command: ' + cmd)
            return
        }
        cmdRequests.push({
            name: cmd,
            resolve: () => { }
        })
        notifyNewCommand()
    }
    document.querySelectorAll('#panel-controller .button-controller').forEach($el => {
        $el.addEventListener('click', commandHandler($el))
    })
    const $albumImg = document.getElementById('image-album')
    const albumImgHandler = commandHandler($albumImg)
    let albumImgLastClick = 0
    const $albumLike = document.getElementById('image-album-like')
    $albumImg.addEventListener('click', e => {
        const now = Date.now()
        const diff = now - albumImgLastClick
        albumImgLastClick = now
        if (diff >= 300) {
            return
        }
        albumImgHandler(e)
        $albumLike.classList.remove('show')
        $albumLike.offsetHeight
        $albumLike.classList.add('show')
    })

    while (true) {
        const cmd = cmdRequests.shift()
        if (!cmd) {
            await new Promise(resolve => notifyNewCommand = resolve)
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

/**
 * @param {string} path 
 */
async function requestMedia(path) {
    const res = await fetch('media/' + path)
    if (!res.ok) {
        throw new Error(`Failed to get media/${path}: ${res.status}`)
    }
    return await res.json()
}
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))
async function mediaLoop() {
    const $playPauseBtn = document.getElementById('button-play-pause')
    /** * @type {HTMLDivElement} */
    const $trackProgressCursor = document.querySelector('#progress-bar-track-progress>.progress-bar-cursor')
    const $albumImage = document.getElementById('image-album')
    const $trackTitle = document.getElementById('text-track-title')
    const $trackArtist = document.getElementById('text-track-artist')
    const $trackAlbum = document.getElementById('text-track-album')

    let lastMediaInfo = {
        title: '',
        artist: '',
        album: '',
        timeline: {
            position: 0,
            duration: 0,
            paused: true
        }
    }
    let lastAlbumImageBlob = ''
    let lastAlbumImageHash = ''
    while (true) {
        /**
         * @type {typeof lastMediaInfo}
         */
        let mediaInfo
        try {
            mediaInfo = await requestMedia('info')
        } catch (e) {
            console.error(e)
            await sleep(3000)
            continue
        }
        if (mediaInfo.timeline.paused) {
            $playPauseBtn.classList.add('paused')
            $playPauseBtn.classList.remove('playing')
        }
        else {
            $playPauseBtn.classList.add('playing')
            $playPauseBtn.classList.remove('paused')
        }
        if (!isNaN(mediaInfo.timeline.duration) && !isNaN(mediaInfo.timeline.position) && mediaInfo.timeline.duration > 0) {
            $trackProgressCursor.style.left = `${mediaInfo.timeline.position / mediaInfo.timeline.duration * 100}%`
        }
        const trackChanged = lastMediaInfo.title !== mediaInfo.title || lastMediaInfo.artist !== mediaInfo.artist || lastMediaInfo.album !== mediaInfo.album
        if (trackChanged) {
            $trackTitle.textContent = mediaInfo.title
            $trackArtist.textContent = mediaInfo.artist
            $trackAlbum.textContent = mediaInfo.album
        }
        const albumChanged = lastMediaInfo.album !== mediaInfo.album
        if (albumChanged) {
            /**
             * @type {{Url: string} | {Blob: {mime: string, base64: string}}}
             */
            let albumImage
            let albumImageHash = lastAlbumImageHash
            do {
                try {
                    albumImage = await requestMedia('album_img')
                } catch (e) {
                    console.error(e)
                    break
                }
                albumImageHash = albumImage?.Url || albumImage?.Blob?.base64 || ''
            } while (albumImageHash === lastAlbumImageHash && (await sleep(1000) || true))
            lastAlbumImageHash = albumImageHash
            if (albumImage) {
                if ('Url' in albumImage) {
                    $albumImage.src = albumImage.Url
                } else {
                    const imageRes = await fetch(`data:${albumImage.Blob.mime};base64,${albumImage.Blob.base64}`)
                    const blob = await imageRes.blob()
                    const url = URL.createObjectURL(new Blob([blob], { type: albumImage.Blob.mime }))
                    $albumImage.src = url
                    if (lastAlbumImageBlob) {
                        URL.revokeObjectURL(lastAlbumImageBlob)
                    }
                    lastAlbumImageBlob = url
                }
                $albumImage.classList.add('loaded')
            } else {
                $albumImage.setAttribute('data-loaded', 'false')
            }
        }
        lastMediaInfo = mediaInfo
        await sleep(1000)
    }
}

mediaLoop()

document.querySelectorAll('#button-volume-up,#button-volume-down').forEach($el => {
    const $volumeLevel = document.getElementById('text-volume-level')

    $el.addEventListener('click', async e => {
        await sleep(50)
        const res = await fetch('media/volume')
        if (!res.ok) {
            alert('Failed to change volume: ' + res.status)
        }
        /**
         * @type {{level: number, muted: boolean}}
         */
        const volume = await res.json()
        $volumeLevel.textContent = `${Math.round(volume.level * 100)}%`
        $volumeLevel.classList.remove('show')
        $volumeLevel.offsetHeight
        $volumeLevel.classList.add('show')
    })
})
