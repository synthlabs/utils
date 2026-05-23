package util

import (
	"reflect"
	"time"

	"github.com/mitchellh/mapstructure"
	"google.golang.org/protobuf/types/known/durationpb"
)

type JSON map[string]any

func Int64ToPbDurationHookFunc() mapstructure.DecodeHookFunc {
	return func(
		f reflect.Type,
		t reflect.Type,
		data interface{},
	) (interface{}, error) {
		if f.Kind() != reflect.Int64 {
			return data, nil
		}
		if t != reflect.TypeOf(durationpb.New(time.Second)) {
			return data, nil
		}

		// if it's a time.Duration
		if dur, ok := data.(time.Duration); ok {
			return durationpb.New(dur), nil
		}

		if dur, ok := data.(int64); ok {
			return durationpb.New(time.Duration(dur)), nil
		}

		// otherwise pass it through
		return data, nil
	}
}

func ConvertToType[F any, T any](from F) (T, error) {
	return convertToType[F, T](from, false)
}

func ConvertToTypeProto[F any, T any](from F) (T, error) {
	return convertToType[F, T](from, true)
}

func convertToType[F any, T any](from F, proto bool) (T, error) {
	var to T

	hooks := []mapstructure.DecodeHookFunc{
		mapstructure.StringToTimeHookFunc(time.RFC3339),
		mapstructure.StringToTimeDurationHookFunc(),
	}

	if proto {
		hooks = append([]mapstructure.DecodeHookFunc{Int64ToPbDurationHookFunc()}, hooks...)
	}

	decoder, err := mapstructure.NewDecoder(&mapstructure.DecoderConfig{
		Metadata:         nil,
		TagName:          "json",
		WeaklyTypedInput: true,
		Result:           &to,
		DecodeHook:       mapstructure.OrComposeDecodeHookFunc(hooks...),
	})
	if err != nil {
		return to, err
	}

	// convert from given type (from) -> T (to)
	err = decoder.Decode(from)

	return to, err
}
