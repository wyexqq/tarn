const LEVELS_PER_SECOND = 16000 / 1024;
const HISTORY_LENGTH = Math.round(LEVELS_PER_SECOND * 5);
const HISTORY_NEEDED_FOR_STRIP = Math.round(LEVELS_PER_SECOND * 0.3);
const HISTORY_NEEDED_FOR_RECOGNITION = Math.round(LEVELS_PER_SECOND * 1.5);
const QUIET_SHARE_OF_HISTORY = 0.15;

const LOWEST_SPEECH_LEVEL_ON_STRIP = 0.012;
const START_OF_SPEECH_OVER_NOISE = 1.8;
const CONTINUED_SPEECH_OVER_NOISE = 1.3;
const LOWEST_VOICE_LEVEL_FOR_RECOGNITION = 0.012;
const RECOGNITION_VOICE_OVER_NOISE = 1.5;

const LOWEST_NOISE_FOR_HEIGHT = 0.003;
const SMALLEST_LOUDNESS_RANGE = 6;
const SMALLEST_LINE_SHARE = 0.05;

export class VoiceNoiseTracker {
  private recentLevels: number[] = [];
  private isSpeakingOnStrip = false;

  add(level: number): void {
    this.recentLevels.push(level);
    if (this.recentLevels.length > HISTORY_LENGTH) {
      this.recentLevels.shift();
    }
    this.isSpeakingOnStrip = this.decideSpeakingOnStrip(level);
  }

  private noiseLevel(): number {
    const ascending = [...this.recentLevels].sort((first, second) => first - second);
    return ascending[Math.floor(ascending.length * QUIET_SHARE_OF_HISTORY)];
  }

  private decideSpeakingOnStrip(level: number): boolean {
    if (this.recentLevels.length < HISTORY_NEEDED_FOR_STRIP || level < LOWEST_SPEECH_LEVEL_ON_STRIP) {
      return false;
    }
    const neededOverNoise = this.isSpeakingOnStrip ? CONTINUED_SPEECH_OVER_NOISE : START_OF_SPEECH_OVER_NOISE;
    return level >= this.noiseLevel() * neededOverNoise;
  }

  stripShareOfLastLevel(): number {
    if (!this.isSpeakingOnStrip) {
      return 0;
    }
    const level = this.recentLevels[this.recentLevels.length - 1];
    const noise = Math.max(this.noiseLevel(), LOWEST_NOISE_FOR_HEIGHT);
    const loudestRecent = Math.max(...this.recentLevels);
    const loudnessRange = Math.max(loudestRecent / noise, SMALLEST_LOUDNESS_RANGE);
    const share = Math.log(level / noise) / Math.log(loudnessRange);
    return Math.min(1, Math.max(SMALLEST_LINE_SHARE, share));
  }

  mayBeVoiceForRecognition(level: number): boolean {
    if (level < LOWEST_VOICE_LEVEL_FOR_RECOGNITION) {
      return false;
    }
    return this.recentLevels.length < HISTORY_NEEDED_FOR_RECOGNITION || level >= this.noiseLevel() * RECOGNITION_VOICE_OVER_NOISE;
  }
}
