/**
 * @param {string} b64 
 */
async function b64StrToBytes(b64) {
    const res = await fetch('data:application/octet-stream;base64,' + b64)
    return new Uint8Array(await res.arrayBuffer())
}

let debugEnableLog = window.location.href === 'https://127-0-0-1.traefik.me:9201/'

const { webSocket: rxWebSocket } = rxjs.webSocket

class KeyExchange {
    /**
     * @param {CryptoKey} privateKey 
     * @param {ArrayBuffer} psk
     */
    constructor(privateKey, psk) {
        this.privateKey = privateKey
        this.psk = psk
    }

    /**
     * @param {string} psk
     */
    static async generate(psk) {
        const pair = await crypto.subtle.generateKey(
            { name: 'ECDH', namedCurve: 'P-256' },
            true,
            ['deriveKey']
        )
        const publicKey = await crypto.subtle.exportKey('raw', pair.publicKey)
        const pskBytes = await b64StrToBytes(psk)
        return { publicKey, keyExchange: new KeyExchange(pair.privateKey, pskBytes) }
    }
    /**
     * @param {ArrayBuffer} serverMaterial 
     */
    async handshake(serverMaterial) {
        const serverPublicKey = await crypto.subtle.importKey(
            'raw',
            serverMaterial,
            { name: 'ECDH', namedCurve: 'P-256' },
            false,
            []
        )
        const sharedSecret = await crypto.subtle.deriveKey(
            { name: 'ECDH', public: serverPublicKey },
            this.privateKey,
            { name: 'HKDF' },
            false,
            ['deriveKey']
        )
        const downloadKey = await crypto.subtle.deriveKey(
            { name: 'HKDF', hash: 'SHA-256', info: new TextEncoder().encode('download'), salt: this.psk },
            sharedSecret,
            { name: 'AES-GCM', length: 128 },
            true,
            ['decrypt']
        )
        const uploadKey = await crypto.subtle.deriveKey(
            { name: 'HKDF', hash: 'SHA-256', info: new TextEncoder().encode('upload'), salt: this.psk },
            sharedSecret,
            { name: 'AES-GCM', length: 128 },
            true,
            ['encrypt']
        )

        return {
            downloadCrypto: new CryptoSession(downloadKey),
            uploadCrypto: new CryptoSession(uploadKey)
        }
    }
}
class CryptoSession {
    /**
     * @param {CryptoKey} key 
     */
    constructor(key) {
        this.key = key
        this.nonce = new Uint8Array(12)
    }

    increaseNonce() {
        let c = 1
        for (let i = 0; i < 12; i++) {
            c += this.nonce[i]
            this.nonce[i] = c & 0xff
            c >>= 8
        }
    }

    /**
     * @param {string} data 
     * @returns {Promise<ArrayBuffer>}
     */
    async encrypt(data) {
        const encrypted = await crypto.subtle.encrypt(
            { name: 'AES-GCM', iv: this.nonce },
            this.key,
            new TextEncoder().encode(data)
        )
        this.increaseNonce()
        return encrypted
    }

    /**
     * @param {ArrayBuffer} data 
     * @returns {Promise<string>}
     */
    async decrypt(data) {
        const decrypted = await crypto.subtle.decrypt(
            { name: 'AES-GCM', iv: this.nonce },
            this.key,
            data
        )
        this.increaseNonce()
        const res = new TextDecoder().decode(decrypted)
        return res
    }
}

const $inputPrivateKey = /** @type {HTMLInputElement} */ (document.getElementById('input-private-key'))
$inputPrivateKey.value = localStorage.getItem('private-key') ?? ''
const createPsk$ = () =>
    rxjs.fromEvent($inputPrivateKey, 'change').pipe(
        rxjs.map(e => $inputPrivateKey.value.trim()),
        rxjs.debounceTime(1000),
        rxjs.distinctUntilChanged(),
        rxjs.tap(psk => localStorage.setItem('private-key', psk)),
        rxjs.startWith(String($inputPrivateKey.value)),
    )

const Commands =
    /** @type {const} */
    (['heartbeat', 'heartbeat_res', 'play_pause', 'prev_track', 'next_track', 'volume_up', 'volume_down', 'like'])
/** @typedef {(typeof Commands)[number]} Commands */
/** @type {Subject<Commands>} */
const heartbeatSubject$ = new rxjs.Subject()
/**
 * @returns {Observable<Commands>}
 */
function createControl$() {
    const buttonCtrl$s = Array.from(document.querySelectorAll('#panel-controller .button-controller'))
        .map(button => rxjs.fromEvent(button, 'click')
            .pipe(rxjs.map(() => {
                const cmdName = button.getAttribute('data-cmd-name') ?? ''
                if (!/** @type {readonly string[]} */(Commands).includes(cmdName)) {
                    new Error(`Invalid command name: ${cmdName}`)
                }
                return /** @type {Commands} */ (cmdName)
            })))
    const $albumLike = document.getElementById('image-album-like')
    const like$ = rxjs.fromEvent(document.getElementById('image-album'), 'click')
        .pipe(
            rxjs.map(() => Date.now()),
            rxjs.startWith(0),
            rxjs.pairwise(),
            rxjs.filter(([prev, curr]) => curr - prev < 500),
            rxjs.map(() => /** @type {'like'} */('like')),
            rxjs.tap(() => {
                $albumLike.classList.remove('show')
                $albumLike.offsetHeight
                $albumLike.classList.add('show')
            }))
    return rxjs.merge(...buttonCtrl$s, like$, heartbeatSubject$)
}

/**
 * @param {Commands} command
 * @returns {string}
 */
function mapCommandToPayload(command) {
    const payload = JSON.stringify({
        heartbeat: 'Heartbeat',
        heartbeat_res: 'HeartbeatRes',
        play_pause: 'TogglePlayPause',
        prev_track: 'PrevTrack',
        next_track: 'NextTrack',
        volume_up: 'VolumeUp',
        volume_down: 'VolumeDown',
        like: 'Like'
    }[command])
    if (debugEnableLog) {
        console.log(`Mapped command ${command} to payload ${payload}`)
    }
    return payload
}

/**
 * @typedef {{heartbeat: null}} WsHeartbeatEvent
 * @typedef {{heartbeat_res: null}} WsHeartbeatResEvent
 * @typedef {{timeline: {duration: number, position: number, paused: boolean}}} WsTimelineStateEvent
 * @typedef {{title: string, artist: string, album: string, timeline: WsTimelineStateEvent['timeline']}} WsTrackInfoEvent
 * @typedef {{album_img: {Blob: {mime: string, base64: string}}}} WsAlbumImageBlobEvent
 * @typedef {{album_img: {Url: string}}} WsAlbumImageUrlEvent
 * @typedef {{album_img: null}} WsAlbumImageNotAvailableEvent
 * {"volume":{"level":0.22,"muted":false}}
 * @typedef {{volume: {level: number, muted: boolean}}} WsVolumeEvent
 * @typedef {{ctx: string, error: string}} WsErrorEvent
 * @typedef {WsHeartbeatEvent|WsHeartbeatResEvent|WsTimelineStateEvent|WsTrackInfoEvent|WsAlbumImageBlobEvent|WsAlbumImageUrlEvent|WsVolumeEvent|WsErrorEvent} WsEvent
 */
/**
 * @param {string} data 
 * @returns {WsEvent}
 */
function deserializeWsEvents(data) {
    return JSON.parse(data)
}

/**
 * @param {string} psk 
 */
function runWs$(psk) {
    const $playPauseBtn = document.getElementById('button-play-pause')
    /** * @type {HTMLDivElement} */
    const $trackProgressCursor = document.querySelector('#progress-bar-track-progress>.progress-bar-cursor')
    const $albumImage = /** @type {HTMLImageElement} */ (document.getElementById('image-album'))
    const $trackTitle = document.getElementById('text-track-title')
    const $trackArtist = document.getElementById('text-track-artist')
    const $trackAlbum = document.getElementById('text-track-album')
    const $volumeLevel = document.getElementById('text-volume-level')

    const ws$ = new rxjs.Observable(
        /**
         * @param {Subscriber<{
         * ws: WebSocket,
         * msg$: Subject<ArrayBuffer>
         * }>} subscriber
         */
        subscriber => {
            const ws = new WebSocket('main_ws')
            if (debugEnableLog) {
                console.log('Connecting to main_ws')
            }
            ws.binaryType = 'arraybuffer'
            /** @type {Subject<ArrayBuffer>} */
            const msg$ = new rxjs.Subject()
            ws.onclose = e => {
                const msg = `Disconnected from main_ws, code: ${e.code}, reason: ${e.reason}`
                if (debugEnableLog) {
                    console.log(msg)
                }
                subscriber.error(new Error(msg))
                msg$.complete()
            }
            ws.onmessage = e => msg$.next(e.data)
            ws.onerror = e => subscriber.error(e)
            ws.onopen = () => {
                if (debugEnableLog) {
                    console.log('Connected to main_ws')
                }
                subscriber.next({ ws, msg$ })
            }
            return () => ws.close()
        })
    return ws$.pipe(
        rxjs.switchMap(ctx => rxjs.from(KeyExchange.generate(psk)).pipe(
            rxjs.tap(({ publicKey }) => ctx.ws.send(publicKey)),
            rxjs.map(({ keyExchange }) => ({ ...ctx, keyExchange }))
        )),
        rxjs.switchMap(ctx => ctx.msg$.pipe(
            rxjs.first(),
            rxjs.map(serverMaterial => ({ ...ctx, serverMaterial }))
        )),
        rxjs.switchMap(ctx => rxjs.from(ctx.keyExchange.handshake(ctx.serverMaterial))
            .pipe(rxjs.map(({ downloadCrypto, uploadCrypto }) => ({
                ws: ctx.ws,
                msg$: ctx.msg$.pipe(
                    rxjs.concatMap(msg => rxjs.from(downloadCrypto.decrypt(msg))),
                    rxjs.share()
                ),
                uploadCrypto
            })))
        ),
        rxjs.switchMap(ctx => rxjs.merge(
            ctx.msg$.pipe(
                rxjs.map(msg => ({
                    ws: ctx.ws,
                    msg: deserializeWsEvents(msg)
                })),
            ),
            createControl$().pipe(
                rxjs.startWith(/** @type {'heartbeat'} */('heartbeat')),
                rxjs.concatMap(cmd => rxjs.from(ctx.uploadCrypto.encrypt(mapCommandToPayload(cmd)))),
                rxjs.map(payload => ({
                    ws: ctx.ws,
                    payload
                })),
            ),
            rxjs.merge(
                ctx.msg$.pipe(
                    rxjs.debounceTime(30 * 1000),
                    rxjs.mergeWith(rxjs.fromEvent(document, 'visibilitychange').pipe(
                        rxjs.filter(() => !document.hidden),
                        rxjs.throttleTime(1000)
                    )),
                    rxjs.tap(() => heartbeatSubject$.next('heartbeat')),
                    rxjs.ignoreElements()
                ),
                ctx.msg$
            ).pipe(
                rxjs.timeout(35 * 1000),
                rxjs.catchError(() => rxjs.throwError(() => new Error('No heartbeat response'))),
                rxjs.ignoreElements()
            )
        )),
        rxjs.tap({
            error: e => {
                if (debugEnableLog) {
                    console.error(e)
                }
            }
        }),
        rxjs.retry({
            delay: (_err, retryCount) => rxjs.timer(retryCount === 1 ? 0 : 3000),
            resetOnSuccess: true
        }),
        rxjs.tap({
            next: ctx => {
                if ('payload' in ctx) {
                    ctx.ws.send(ctx.payload)
                } else {
                    if (debugEnableLog) {
                        console.log(`Received event: ${JSON.stringify(ctx.msg)}`)
                    }
                    if ('heartbeat' in ctx.msg) {
                        heartbeatSubject$.next('heartbeat_res')
                    }
                    if ('title' in ctx.msg) {
                        $trackTitle.textContent = ctx.msg.title
                        $trackArtist.textContent = ctx.msg.artist
                        $trackAlbum.textContent = ctx.msg.album
                    }
                    if ('timeline' in ctx.msg) {
                        const { duration, position, paused } = ctx.msg.timeline
                        if (!isNaN(duration) && !isNaN(position) && duration > 0) {
                            $trackProgressCursor.style.left = `${position / duration * 100}%`
                        }
                        if (paused) {
                            $playPauseBtn.classList.add('paused')
                            $playPauseBtn.classList.remove('playing')
                        } else {
                            $playPauseBtn.classList.add('playing')
                            $playPauseBtn.classList.remove('paused')
                        }
                    }
                    if ('album_img' in ctx.msg) {
                        $albumImage.classList.add('loaded')
                        if (ctx.msg.album_img === null) {
                            $albumImage.classList.remove('loaded')
                            $albumImage.src = ''
                        } else if ('Blob' in ctx.msg.album_img) {
                            const { mime, base64 } = ctx.msg.album_img.Blob
                            $albumImage.src = `data:${mime};base64,${base64}`
                        } else if ('Url' in ctx.msg.album_img) {
                            $albumImage.src = ctx.msg.album_img.Url
                        }
                    }
                    if ('volume' in ctx.msg) {
                        $volumeLevel.textContent = `${Math.round(ctx.msg.volume.level * 100)}%`
                        $volumeLevel.classList.remove('show')
                        $volumeLevel.offsetHeight
                        $volumeLevel.classList.add('show')
                    }
                    if ('error' in ctx.msg) {
                        alert(JSON.stringify(ctx.msg))
                    }
                }
            },
        })
    )
}

function subscribeWsOnPsk() {
    createPsk$().pipe(
        rxjs.filter(psk => Boolean(psk)),
        rxjs.switchMap(psk => runWs$(psk))
    ).subscribe({})
}

subscribeWsOnPsk()


const $debugEnableLog = /** @type {HTMLInputElement} */ (document.getElementById('enable-log'))
$debugEnableLog.addEventListener('change', e => {
    console.log('Debug log: ' + String($debugEnableLog.checked))
    debugEnableLog = $debugEnableLog.checked
})
