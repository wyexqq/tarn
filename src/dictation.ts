import { api } from "./api";
import type { RefinedGroup } from "./dictationText";
import { state } from "./store";
import { SAMPLE_RATE, startRecording, type Recording } from "./voice";
import { allReplacements, applyReplacements } from "./voiceDictionary";
import { VoiceNoiseTracker } from "./voiceNoise";

const TICK_MS = 100;
const PAUSE_SAMPLES = SAMPLE_RATE * 0.6;
const LEAD_IN_SAMPLES = SAMPLE_RATE * 0.5;
const MIN_AUDIO_SAMPLES = SAMPLE_RATE * 0.6;
const MIN_NEW_AUDIO_SAMPLES = SAMPLE_RATE * 0.35;
const MAX_SEGMENT_SAMPLES = SAMPLE_RATE * 6;
const QUIET_SEARCH_SAMPLES = SAMPLE_RATE * 2;
const REFINE_SPAN_SAMPLES = SAMPLE_RATE * 12;
const MAX_SAMPLES_WITHOUT_WORDS = SAMPLE_RATE * 30;

function withDictionary(text: string): string {
  return applyReplacements(text, allReplacements(state.settings().voiceReplacements));
}

interface SpeechSegment {
  id: number;
  startSample: number;
  endSample: number;
  shownUpTo: number;
  lastVoiceSample: number;
  isEdited: boolean;
  isRefined: boolean;
}

interface LevelMark {
  endSample: number;
  level: number;
}

export interface DictationCallbacks {
  onLevel: (level: number) => void;
  onSegmentText: (segmentId: number, text: string) => void;
  onShouldStop: () => void;
}

export interface Dictation {
  noteEditedSegments: (segmentIds: number[]) => void;
  noteUserTyping: () => void;
  finish: () => Promise<RefinedGroup[]>;
  cancel: () => void;
}

export async function startDictation(callbacks: DictationCallbacks): Promise<Dictation> {
  let lastVoiceSample = -1;
  let recentLevels: LevelMark[] = [];
  const noise = new VoiceNoiseTracker();

  const rememberLevel = (level: number, sampleCount: number) => {
    noise.add(level);
    callbacks.onLevel(noise.stripShareOfLastLevel());
    recentLevels.push({ endSample: sampleCount, level });
    recentLevels = recentLevels.filter((mark) => mark.endSample >= sampleCount - QUIET_SEARCH_SAMPLES);
    if (noise.mayBeVoiceForRecognition(level)) {
      lastVoiceSample = sampleCount;
    }
  };

  const recording: Recording = await startRecording(rememberLevel, callbacks.onShouldStop);

  let nextSegmentId = 1;
  const newSegment = (startSample: number): SpeechSegment => ({
    id: nextSegmentId++,
    startSample,
    endSample: startSample,
    shownUpTo: startSample,
    lastVoiceSample: -1,
    isEdited: false,
    isRefined: false,
  });

  const closedSegments: SpeechSegment[] = [];
  let openSegment = newSegment(0);
  let isListening = true;
  let isCancelled = false;
  let isLiveBroken = false;
  let hasShownText = false;
  let firstError: unknown = null;
  let livePass: Promise<void> | null = null;
  let refinePass: Promise<void> | null = null;
  const refinedGroups: RefinedGroup[] = [];
  const shownSegmentTexts = new Map<number, string>();
  let lastActivitySample = 0;

  const openHasVoice = () => lastVoiceSample > openSegment.startSample;

  const closeOpenSegmentAt = (endSample: number) => {
    openSegment.endSample = endSample;
    openSegment.lastVoiceSample = Math.min(lastVoiceSample, endSample);
    closedSegments.push(openSegment);
    openSegment = newSegment(endSample);
  };

  const quietestRecentSample = () => {
    const quietest = recentLevels.reduce((best, mark) => (mark.level < best.level ? mark : best), recentLevels[0]);
    return quietest.endSample;
  };

  const updateOpenSegment = () => {
    const now = recording.sampleCount();
    if (!openHasVoice()) {
      openSegment.startSample = Math.max(openSegment.startSample, now - LEAD_IN_SAMPLES);
      openSegment.shownUpTo = openSegment.startSample;
    } else if (now - lastVoiceSample >= PAUSE_SAMPLES) {
      closeOpenSegmentAt(now);
    } else if (now - openSegment.startSample >= MAX_SEGMENT_SAMPLES && recentLevels.length > 0) {
      closeOpenSegmentAt(quietestRecentSample());
    }
  };

  const closedSegmentNeedingText = () => closedSegments.find((segment) => !segment.isEdited && segment.lastVoiceSample > segment.shownUpTo);

  const startLivePass = (segment: SpeechSegment, endSample: number) => {
    livePass = api
      .transcribeAudio({ wavBase64: recording.audioBetween(segment.startSample, endSample), isLive: true })
      .then(withDictionary)
      .then((text) => {
        segment.shownUpTo = endSample;
        if (!isCancelled) {
          hasShownText = hasShownText || text.length > 0;
          if (text.length > 0 && shownSegmentTexts.get(segment.id) !== text) {
            shownSegmentTexts.set(segment.id, text);
            lastActivitySample = recording.sampleCount();
          }
          callbacks.onSegmentText(segment.id, text);
        }
      })
      .catch((error) => {
        firstError = firstError ?? error;
        isLiveBroken = true;
      })
      .finally(() => {
        livePass = null;
      });
  };

  const startNextLivePass = () => {
    if (livePass || isLiveBroken) {
      return;
    }
    const closed = closedSegmentNeedingText();
    if (closed) {
      startLivePass(closed, closed.endSample);
      return;
    }
    const now = recording.sampleCount();
    const hasEnoughAudio = now - openSegment.startSample >= MIN_AUDIO_SAMPLES;
    const hasNewAudio = now - openSegment.shownUpTo >= MIN_NEW_AUDIO_SAMPLES;
    const hasNewVoice = lastVoiceSample > openSegment.shownUpTo;
    if (hasEnoughAudio && hasNewAudio && hasNewVoice) {
      startLivePass(openSegment, now);
    }
  };

  const nextSegmentsToRefine = () => {
    const run: SpeechSegment[] = [];
    for (const segment of closedSegments) {
      if (segment.isRefined) {
        continue;
      }
      if (segment.isEdited) {
        if (run.length > 0) {
          break;
        }
        segment.isRefined = true;
        continue;
      }
      run.push(segment);
    }
    return run;
  };

  const startRefining = (segments: SpeechSegment[]) => {
    const startSample = segments[0].startSample;
    const endSample = segments[segments.length - 1].endSample;
    segments.forEach((segment) => {
      segment.isRefined = true;
    });
    refinePass = api
      .transcribeAudio({ wavBase64: recording.audioBetween(startSample, endSample), isLive: false })
      .then(withDictionary)
      .then((text) => {
        refinedGroups.push({ segmentIds: segments.map((segment) => segment.id), text });
      })
      .catch(() => undefined)
      .finally(() => {
        refinePass = null;
      });
  };

  const startRefiningWhenEnoughIsSpoken = () => {
    if (refinePass) {
      return;
    }
    const segments = nextSegmentsToRefine();
    if (segments.length < 2) {
      return;
    }
    const span = segments[segments.length - 1].endSample - segments[0].startSample;
    if (span >= REFINE_SPAN_SAMPLES) {
      startRefining(segments);
    }
  };

  const listen = async () => {
    while (isListening) {
      await new Promise((resolve) => window.setTimeout(resolve, TICK_MS));
      if (isListening && recording.sampleCount() - lastActivitySample >= MAX_SAMPLES_WITHOUT_WORDS) {
        callbacks.onShouldStop();
        return;
      }
      if (isListening) {
        updateOpenSegment();
        startNextLivePass();
        startRefiningWhenEnoughIsSpoken();
      }
    }
  };
  const listening = listen();

  const showRemainingSegments = async () => {
    await livePass;
    let segment = closedSegmentNeedingText();
    while (segment && !isLiveBroken && !isCancelled) {
      startLivePass(segment, segment.endSample);
      await livePass;
      segment = closedSegmentNeedingText();
    }
  };

  const refineRemainingSegments = async () => {
    await refinePass;
    let segments = nextSegmentsToRefine();
    while (segments.length > 1 && !isCancelled) {
      startRefining(segments);
      await refinePass;
      segments = nextSegmentsToRefine();
    }
  };

  const finish = async () => {
    isListening = false;
    const now = recording.sampleCount();
    recording.stop();
    await listening;
    if (lastVoiceSample < 0) {
      openSegment.startSample = 0;
      openSegment.shownUpTo = 0;
      lastVoiceSample = now;
    }
    if (openHasVoice()) {
      closeOpenSegmentAt(now);
    }
    await Promise.all([showRemainingSegments(), refineRemainingSegments()]);
    if (!hasShownText && firstError) {
      throw firstError;
    }
    return refinedGroups;
  };

  return {
    noteEditedSegments: (segmentIds) => {
      closedSegments.filter((segment) => segmentIds.includes(segment.id)).forEach((segment) => {
        segment.isEdited = true;
      });
      if (segmentIds.includes(openSegment.id)) {
        openSegment.isEdited = true;
        closeOpenSegmentAt(openSegment.shownUpTo);
      }
    },
    noteUserTyping: () => {
      lastActivitySample = recording.sampleCount();
    },
    finish,
    cancel: () => {
      isCancelled = true;
      isListening = false;
      recording.cancel();
    },
  };
}
