import EventEmitter from 'eventemitter3';

interface EventTypes {
  status: (opened: boolean, content?: string) => void;

  replaceText: (text: string, highlight: boolean | number) => void;

  inputFailedValidation: (x: number, y: number, validationId: string) => void;

  valueChanged: (value: string) => void;
}

export const inlineEditorEvents = new EventEmitter<EventTypes>();
