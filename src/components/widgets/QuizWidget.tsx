import { For, Index, Show, createMemo, createSignal } from "solid-js";

import { sendMessage } from "../../store";
import type { QuizQuestion } from "../../widgets/types";
import { QuizKindIcon, RestartIcon } from "../icons";
import { WidgetAction, WidgetCard } from "./WidgetCard";

const LETTERS = ["А", "Б", "В", "Г", "Д", "Е"];

interface Answer {
  picked: number;
  isRight: boolean;
}

function optionState(question: QuizQuestion, index: number, picked: number | null, checked: boolean): string {
  if (checked && index === question.answer) {
    return "is-right";
  }
  if (checked && index === picked) {
    return "is-wrong";
  }
  return index === picked ? "is-picked" : "";
}

function resultText(title: string, questions: QuizQuestion[], answers: Answer[]): string {
  const right = answers.filter((answer) => answer.isRight).length;
  const lines = [`Результат теста «${title}»: ${right} из ${questions.length}.`];
  answers.forEach((answer, index) => {
    if (answer.isRight) {
      return;
    }
    const question = questions[index];
    lines.push(`Ошибка в вопросе «${question.question}»: выбрано «${question.options[answer.picked]}», верно «${question.options[question.answer]}».`);
  });
  return lines.join("\n");
}

export function QuizWidget(props: { title: string; questions: QuizQuestion[] }) {
  const [current, setCurrent] = createSignal(0);
  const [picked, setPicked] = createSignal<number | null>(null);
  const [checked, setChecked] = createSignal(false);
  const [answers, setAnswers] = createSignal<Answer[]>([]);
  const [sent, setSent] = createSignal(false);

  const total = () => props.questions.length;
  const isFinished = () => current() >= total();
  const question = createMemo(() => props.questions[Math.min(current(), total() - 1)]);
  const rightCount = () => answers().filter((answer) => answer.isRight).length;

  const restart = () => {
    setCurrent(0);
    setPicked(null);
    setChecked(false);
    setAnswers([]);
    setSent(false);
  };

  const check = () => {
    const choice = picked();
    if (choice === null) {
      return;
    }
    setChecked(true);
    setAnswers([...answers(), { picked: choice, isRight: choice === question().answer }]);
  };

  const next = () => {
    setCurrent(current() + 1);
    setPicked(null);
    setChecked(false);
  };

  const sendResult = async () => {
    const delivered = await sendMessage(resultText(props.title, props.questions, answers()), []);
    setSent(delivered);
  };

  const dotState = (index: number) => {
    if (index < current()) {
      return answers()[index]?.isRight ? "is-done" : "is-missed";
    }
    return index === current() ? "is-current" : "";
  };

  return (
    <WidgetCard
      icon={<QuizKindIcon />}
      title={props.title}
      actions={
        <WidgetAction label="Начать заново" onClick={restart}>
          <RestartIcon />
        </WidgetAction>
      }
    >
      <div class="quiz">
        <div class="quiz-top">
          <div class="quiz-dots" aria-hidden="true">
            <Index each={props.questions}>{(_, index) => <i class={dotState(index)} />}</Index>
          </div>
          <span class="quiz-count">{isFinished() ? "Готово" : `${current() + 1} из ${total()}`}</span>
        </div>
        <Show
          when={!isFinished()}
          fallback={
            <div class="quiz-score">
              <div class="quiz-score-big">
                {rightCount()} <span>из {total()}</span>
              </div>
              <ul class="quiz-review">
                <For each={props.questions}>
                  {(item, index) => <li classList={{ "is-miss": !answers()[index()]?.isRight }}>{item.question}</li>}
                </For>
              </ul>
              <div class="quiz-foot">
                <button class="ghost-button" onClick={restart}>Пройти ещё раз</button>
                <button class="primary-button" disabled={sent()} onClick={() => void sendResult()}>
                  {sent() ? "Отправлено" : "Отправить результат в чат"}
                </button>
              </div>
            </div>
          }
        >
          <div class="quiz-step">
            <p class="quiz-question">{question().question}</p>
            <div class="quiz-options" role="radiogroup" aria-label={question().question}>
              <Index each={question().options}>
                {(option, index) => (
                  <button
                    class={`quiz-option ${optionState(question(), index, picked(), checked())}`}
                    role="radio"
                    aria-checked={picked() === index}
                    disabled={checked()}
                    onClick={() => setPicked(index)}
                  >
                    <b>{LETTERS[index] ?? index + 1}</b>
                    <span>{option()}</span>
                  </button>
                )}
              </Index>
            </div>
            <Show when={checked()}>
              <p class="quiz-explain">
                <strong>{picked() === question().answer ? "Верно." : "Не совсем."}</strong> {question().explain ?? ""}
              </p>
            </Show>
            <div class="quiz-foot">
              <Show
                when={checked()}
                fallback={
                  <button class="primary-button" disabled={picked() === null} onClick={check}>
                    Проверить
                  </button>
                }
              >
                <button class="primary-button" onClick={next}>
                  {current() + 1 === total() ? "Итог" : "Далее"}
                </button>
              </Show>
            </div>
          </div>
        </Show>
      </div>
    </WidgetCard>
  );
}
