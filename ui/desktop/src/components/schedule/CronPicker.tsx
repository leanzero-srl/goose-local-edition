import React, { useState, useEffect } from 'react';
import type { ScheduledJobDto } from '@aaif/goose-sdk';
import { errorMessage } from '../../utils/conversionUtils';
import { defineMessages, useIntl } from '../../i18n';
import { Select } from '../ui/Select';
import {
  buildCronForPeriod,
  describeCron,
  getQuarterStartMonth,
  getValidDayOfMonth,
  parseCron,
  quarterDayLimitByStartMonth,
  type Period,
} from '../../utils/cronSchedule';

const i18n = defineMessages({
  every: { id: 'cronPicker.every', defaultMessage: 'Every' },
  mode: { id: 'cronPicker.mode', defaultMessage: 'Mode' },
  minute: { id: 'cronPicker.minute', defaultMessage: 'Minute' },
  hour: { id: 'cronPicker.hour', defaultMessage: 'Hour' },
  day: { id: 'cronPicker.day', defaultMessage: 'Day' },
  week: { id: 'cronPicker.week', defaultMessage: 'Week' },
  month: { id: 'cronPicker.month', defaultMessage: 'Month' },
  quarter: { id: 'cronPicker.quarter', defaultMessage: 'Quarter' },
  year: { id: 'cronPicker.year', defaultMessage: 'Year' },
  custom: { id: 'cronPicker.custom', defaultMessage: 'Custom cron' },
  cronExpression: { id: 'cronPicker.cronExpression', defaultMessage: 'Cron expression' },
  emptyCronError: {
    id: 'cronPicker.emptyCronError',
    defaultMessage: 'Cron expression cannot be empty',
  },
  invalidDayOfMonth: {
    id: 'cronPicker.invalidDayOfMonth',
    defaultMessage: 'Day must be between 1 and {max}',
  },
  inMonth: { id: 'cronPicker.inMonth', defaultMessage: 'in' },
  startingMonth: { id: 'cronPicker.startingMonth', defaultMessage: 'starting month' },
  january: { id: 'cronPicker.january', defaultMessage: 'January' },
  february: { id: 'cronPicker.february', defaultMessage: 'February' },
  march: { id: 'cronPicker.march', defaultMessage: 'March' },
  april: { id: 'cronPicker.april', defaultMessage: 'April' },
  may: { id: 'cronPicker.may', defaultMessage: 'May' },
  june: { id: 'cronPicker.june', defaultMessage: 'June' },
  july: { id: 'cronPicker.july', defaultMessage: 'July' },
  august: { id: 'cronPicker.august', defaultMessage: 'August' },
  september: { id: 'cronPicker.september', defaultMessage: 'September' },
  october: { id: 'cronPicker.october', defaultMessage: 'October' },
  november: { id: 'cronPicker.november', defaultMessage: 'November' },
  december: { id: 'cronPicker.december', defaultMessage: 'December' },
  onDay: { id: 'cronPicker.onDay', defaultMessage: 'on day' },
  on: { id: 'cronPicker.on', defaultMessage: 'on' },
  sunday: { id: 'cronPicker.sunday', defaultMessage: 'Sunday' },
  monday: { id: 'cronPicker.monday', defaultMessage: 'Monday' },
  tuesday: { id: 'cronPicker.tuesday', defaultMessage: 'Tuesday' },
  wednesday: { id: 'cronPicker.wednesday', defaultMessage: 'Wednesday' },
  thursday: { id: 'cronPicker.thursday', defaultMessage: 'Thursday' },
  friday: { id: 'cronPicker.friday', defaultMessage: 'Friday' },
  saturday: { id: 'cronPicker.saturday', defaultMessage: 'Saturday' },
  at: { id: 'cronPicker.at', defaultMessage: 'at' },
  atMinute: { id: 'cronPicker.atMinute', defaultMessage: 'at minute' },
  atSecond: { id: 'cronPicker.atSecond', defaultMessage: 'at second' },
});

type SelectOption = { value: string; label: string };

interface CronPickerProps {
  schedule: ScheduledJobDto | null;
  onChange: (cron: string) => void;
  isValid: (valid: boolean) => void;
}

const to24Hour = (hour12: number, isPM: boolean): number => {
  if (hour12 === 12) {
    return isPM ? 12 : 0;
  }
  return isPM ? hour12 + 12 : hour12;
};

const to12Hour = (hour24: number): { hour: number; isPM: boolean } => {
  if (hour24 === 0) {
    return { hour: 12, isPM: false };
  }
  if (hour24 === 12) {
    return { hour: 12, isPM: true };
  }
  if (hour24 > 12) {
    return { hour: hour24 - 12, isPM: true };
  }
  return { hour: hour24, isPM: false };
};

export const CronPicker: React.FC<CronPickerProps> = ({ schedule, onChange, isValid }) => {
  const intl = useIntl();
  const [period, setPeriod] = useState<Period>('day');
  const [second, setSecond] = useState('0');
  const [minute, setMinute] = useState('0');
  const [hour12, setHour12] = useState(2);
  const [isPM, setIsPM] = useState(true);
  const [dayOfWeek, setDayOfWeek] = useState('1');
  const [dayOfMonth, setDayOfMonth] = useState('1');
  const [month, setMonth] = useState('1');
  const [quarterStartMonth, setQuarterStartMonth] = useState('1');
  const [customCron, setCustomCron] = useState('0 0 14 * * *');
  const [readableCron, setReadableCron] = useState('');
  const [hasCronError, setHasCronError] = useState(false);

  const getCurrentCron = (selectedPeriod: Period, validDayOfMonth: string | null): string =>
    buildCronForPeriod({
      period: selectedPeriod,
      second,
      minute,
      hour24: to24Hour(hour12, isPM),
      dayOfWeek,
      dayOfMonth: validDayOfMonth,
      month,
      quarterStartMonth,
      customCron,
    });

  useEffect(() => {
    const sourceCron = schedule?.cron || '';
    const parsed = parseCron(sourceCron);
    setPeriod(parsed.period);
    setSecond(parsed.second === '*' ? '0' : parsed.second);
    setMinute(parsed.minute === '*' ? '0' : parsed.minute);
    const hour24 = parsed.hour === '*' ? 14 : parseInt(parsed.hour, 10);
    const { hour, isPM: pm } = to12Hour(hour24);
    setHour12(hour);
    setIsPM(pm);
    setDayOfWeek(parsed.dayOfWeek === '*' ? '1' : parsed.dayOfWeek);
    setDayOfMonth(parsed.dayOfMonth === '*' ? '1' : parsed.dayOfMonth);
    setMonth(parsed.month === '*' ? '1' : parsed.month);
    setQuarterStartMonth(getQuarterStartMonth(parsed.month) ?? '1');
    setCustomCron(sourceCron || '0 0 14 * * *');
  }, [schedule]);

  const maxDayOfMonth = period === 'quarter' ? quarterDayLimitByStartMonth[quarterStartMonth] : 31;

  useEffect(() => {
    const parsedDay = parseInt(dayOfMonth, 10);
    if (!Number.isNaN(parsedDay) && parsedDay > maxDayOfMonth) {
      setDayOfMonth(maxDayOfMonth.toString());
    }
  }, [dayOfMonth, maxDayOfMonth]);

  useEffect(() => {
    const validDayOfMonth = getValidDayOfMonth(dayOfMonth, maxDayOfMonth);

    if (
      (period === 'month' || period === 'quarter' || period === 'year') &&
      validDayOfMonth === null
    ) {
      onChange(getCurrentCron(period, null));
      isValid(false);
      setHasCronError(true);
      setReadableCron(intl.formatMessage(i18n.invalidDayOfMonth, { max: maxDayOfMonth }));
      return;
    }

    const cron = getCurrentCron(period, validDayOfMonth);
    onChange(cron);
    if (!cron.trim()) {
      isValid(false);
      setHasCronError(true);
      setReadableCron(intl.formatMessage(i18n.emptyCronError));
      return;
    }
    try {
      setReadableCron(describeCron(cron));
      setHasCronError(false);
      isValid(true);
    } catch (e) {
      isValid(false);
      setHasCronError(true);
      setReadableCron(errorMessage(e).replace(/^Error:\s*/, ''));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    period,
    second,
    minute,
    hour12,
    isPM,
    dayOfWeek,
    dayOfMonth,
    month,
    quarterStartMonth,
    maxDayOfMonth,
    customCron,
  ]);

  const periodOptions: SelectOption[] = [
    { value: 'minute', label: intl.formatMessage(i18n.minute) },
    { value: 'hour', label: intl.formatMessage(i18n.hour) },
    { value: 'day', label: intl.formatMessage(i18n.day) },
    { value: 'week', label: intl.formatMessage(i18n.week) },
    { value: 'month', label: intl.formatMessage(i18n.month) },
    { value: 'quarter', label: intl.formatMessage(i18n.quarter) },
    { value: 'year', label: intl.formatMessage(i18n.year) },
    { value: 'custom', label: intl.formatMessage(i18n.custom) },
  ];
  const monthOptions: SelectOption[] = [
    { value: '1', label: intl.formatMessage(i18n.january) },
    { value: '2', label: intl.formatMessage(i18n.february) },
    { value: '3', label: intl.formatMessage(i18n.march) },
    { value: '4', label: intl.formatMessage(i18n.april) },
    { value: '5', label: intl.formatMessage(i18n.may) },
    { value: '6', label: intl.formatMessage(i18n.june) },
    { value: '7', label: intl.formatMessage(i18n.july) },
    { value: '8', label: intl.formatMessage(i18n.august) },
    { value: '9', label: intl.formatMessage(i18n.september) },
    { value: '10', label: intl.formatMessage(i18n.october) },
    { value: '11', label: intl.formatMessage(i18n.november) },
    { value: '12', label: intl.formatMessage(i18n.december) },
  ];
  const quarterMonthOptions: SelectOption[] = monthOptions.slice(0, 3);
  const dayOfWeekOptions: SelectOption[] = [
    { value: '0', label: intl.formatMessage(i18n.sunday) },
    { value: '1', label: intl.formatMessage(i18n.monday) },
    { value: '2', label: intl.formatMessage(i18n.tuesday) },
    { value: '3', label: intl.formatMessage(i18n.wednesday) },
    { value: '4', label: intl.formatMessage(i18n.thursday) },
    { value: '5', label: intl.formatMessage(i18n.friday) },
    { value: '6', label: intl.formatMessage(i18n.saturday) },
  ];
  const amPmOptions: SelectOption[] = [
    { value: 'AM', label: 'AM' },
    { value: 'PM', label: 'PM' },
  ];

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <span className="text-sm font-medium">
          {intl.formatMessage(period === 'custom' ? i18n.mode : i18n.every)}
        </span>
        <div className="w-40">
          <Select
            options={periodOptions}
            value={periodOptions.find((o) => o.value === period) ?? null}
            onChange={(opt) => {
              const nextPeriod = (opt as SelectOption).value as Period;
              if (nextPeriod === 'custom' && period !== 'custom') {
                setCustomCron(getCurrentCron(period, getValidDayOfMonth(dayOfMonth, maxDayOfMonth)));
              }
              setPeriod(nextPeriod);
            }}
            isSearchable={false}
            menuPortalTarget={document.body}
          />
        </div>
      </div>

      <div className="space-y-3">
        {period === 'custom' && (
          <div className="space-y-1">
            <label htmlFor="custom-cron-expression" className="text-sm">
              {intl.formatMessage(i18n.cronExpression)}
            </label>
            <input
              id="custom-cron-expression"
              type="text"
              value={customCron}
              onChange={(e) => setCustomCron(e.target.value)}
              className="w-full px-2 py-1 border rounded"
            />
          </div>
        )}

        {period === 'quarter' && (
          <div className="flex flex-wrap items-center gap-2">
            <div className="flex items-center gap-2">
              <span className="text-sm">{intl.formatMessage(i18n.startingMonth)}</span>
              <div className="w-40">
                <Select
                  options={quarterMonthOptions}
                  value={quarterMonthOptions.find((o) => o.value === quarterStartMonth) ?? null}
                  onChange={(opt) => setQuarterStartMonth((opt as SelectOption).value)}
                  isSearchable={false}
                  menuPortalTarget={document.body}
                />
              </div>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-sm">{intl.formatMessage(i18n.onDay)}</span>
              <input
                type="number"
                min="1"
                max={maxDayOfMonth}
                value={dayOfMonth}
                onChange={(e) => setDayOfMonth(e.target.value)}
                className="w-16 px-2 py-1 border rounded"
              />
            </div>
          </div>
        )}

        {period === 'year' && (
          <div className="flex items-center gap-2">
            <span className="text-sm">{intl.formatMessage(i18n.inMonth)}</span>
            <div className="w-40">
              <Select
                options={monthOptions}
                value={monthOptions.find((o) => o.value === month) ?? null}
                onChange={(opt) => setMonth((opt as SelectOption).value)}
                isSearchable={false}
                menuPortalTarget={document.body}
              />
            </div>
          </div>
        )}

        {(period === 'month' || period === 'year') && (
          <div className="flex items-center gap-2">
            <span className="text-sm">{intl.formatMessage(i18n.onDay)}</span>
            <input
              type="number"
              min="1"
              max={maxDayOfMonth}
              value={dayOfMonth}
              onChange={(e) => setDayOfMonth(e.target.value)}
              className="w-16 px-2 py-1 border rounded"
            />
          </div>
        )}

        {period === 'week' && (
          <div className="flex items-center gap-2">
            <span className="text-sm">{intl.formatMessage(i18n.on)}</span>
            <div className="w-40">
              <Select
                options={dayOfWeekOptions}
                value={dayOfWeekOptions.find((o) => o.value === dayOfWeek) ?? null}
                onChange={(opt) => setDayOfWeek((opt as SelectOption).value)}
                isSearchable={false}
                menuPortalTarget={document.body}
              />
            </div>
          </div>
        )}

        {(period === 'day' ||
          period === 'week' ||
          period === 'month' ||
          period === 'quarter' ||
          period === 'year') && (
          <div className="flex items-center gap-2">
            <span className="text-sm">{intl.formatMessage(i18n.at)}</span>
            <input
              type="number"
              min="1"
              max="12"
              value={hour12}
              onChange={(e) => setHour12(parseInt(e.target.value) || 1)}
              className="w-16 px-2 py-1 border rounded"
            />
            <span className="text-sm">:</span>
            <input
              type="number"
              min="0"
              max="59"
              value={minute}
              onChange={(e) => setMinute(e.target.value.padStart(2, '0'))}
              className="w-16 px-2 py-1 border rounded"
            />
            <div className="w-24">
              <Select
                options={amPmOptions}
                value={amPmOptions.find((o) => o.value === (isPM ? 'PM' : 'AM')) ?? null}
                onChange={(opt) => setIsPM((opt as SelectOption).value === 'PM')}
                isSearchable={false}
                menuPortalTarget={document.body}
              />
            </div>
          </div>
        )}

        {period === 'hour' && (
          <div className="flex items-center gap-2">
            <span className="text-sm">{intl.formatMessage(i18n.atMinute)}</span>
            <input
              type="number"
              min="0"
              max="59"
              value={minute}
              onChange={(e) => setMinute(e.target.value)}
              className="w-16 px-2 py-1 border rounded"
            />
          </div>
        )}

        {period === 'minute' && (
          <div className="flex items-center gap-2">
            <span className="text-sm">{intl.formatMessage(i18n.atSecond)}</span>
            <input
              type="number"
              min="0"
              max="59"
              value={second}
              onChange={(e) => setSecond(e.target.value)}
              className="w-16 px-2 py-1 border rounded"
            />
          </div>
        )}
      </div>

      <div className={`text-xs mt-2 ${hasCronError ? 'text-text-danger' : 'text-gray-500'}`}>
        {readableCron}
      </div>
    </div>
  );
};
